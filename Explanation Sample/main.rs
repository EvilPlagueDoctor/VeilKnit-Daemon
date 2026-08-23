use std::io::{self, Write};
use std::sync::Arc;

mod dht_module;
mod handshake;
mod node;
mod node_list;
mod route_manager;
mod types;
mod user_auth;
mod user_dht;
mod walk_task;

use dht_module::{DHTModule, StoredDhtRecord};
use user_dht::DHT_SNAPSHOT_KEY;
use handshake::HandshakeManager;
use node::*;
use route_manager::RouteManager;
use user_auth::{AuthError, UserAuth, UserSession};
use walk_task::{WalkConfig, WalkHandle, WalkStartResult, WalkStatus, WalkTask, WalkTaskInit};
use veilid_core::RecordKey;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let auth = Arc::new(UserAuth::new("./user_data")?);

    let session = Arc::new(login_or_signup(&auth));

    println!("Welcome, {}!", session.username());

    // Namespace the Veilid node per-account, using the username instead of
    // an arbitrary string, so each user's protected/table store is isolated.
    let node = create_node(session.username().to_string()).await?;

    let veilid = Arc::new(node.veilid.clone());
    let background = DHTModule::new(veilid.clone());

    // Route manager: publishes a private route blob into a DHT so other
    // peers have somewhere stable to look us up. It needs both the node
    // (to actually create/publish routes) and a DHT package index (to know
    // where to write the blob) before it'll do anything.
    let route_manager = RouteManager::spawn();
    node.set_route_change_handler(route_manager.make_route_change_handler());
    route_manager.set_node(node.clone()).await;

    // Try to restore any DHTs this user saved in a previous session.
    match auth.read_user_encrypted::<Vec<StoredDhtRecord>>(&session, DHT_SNAPSHOT_KEY) {
        Ok(Some(snapshot)) if !snapshot.is_empty() => {
            println!("Restoring {} saved DHT(s)...", snapshot.len());
            match background.import_snapshot(snapshot).await {
                Ok(()) => println!("DHTs restored."),
                Err(err) => println!("Failed to restore DHTs: {:?}", err),
            }
        }
        Ok(_) => println!("No saved DHTs found for this account."),
        Err(err) => println!("Could not read saved DHTs: {:?}", err),
    }

    // user_dht owns the main-DHT layout and setup policy. It either reconnects
    // the restored package or creates it, initializes every subkey through the
    // normal DHTModule write API, persists it, and hands it to RouteManager.
    let main_dht_index: Option<usize> = match user_dht::load_or_create_main_dht(
        &auth,
        &session,
        &background,
        &route_manager,
    )
    .await
    {
        Ok(index) => {
            println!("Main DHT is ready at package index {index}.");
            Some(index)
        }
        Err(error) => {
            println!("Main DHT setup failed: {error}");
            None
        }
    };

    // Handshake manager: needs to know our own DHT record key (as a string)
    // so peers can find their way back to us, plus the DHT module (to look
    // up peers' route blobs) and the node (to install the app-message
    // handler and drive retries/check-ins on a timer).
    let our_dht_key = match main_dht_index {
        Some(idx) => match background.get_dht_info(idx).await {
            Some(package) => package.dht_record.key().to_string(),
            None => {
                println!("Warning: route DHT index {idx} vanished; handshakes will report an empty sender address.");
                String::new()
            }
        },
        None => {
            println!("Warning: no route DHT available; handshakes will report an empty sender address.");
            String::new()
        }
    };

    let handshake_manager = HandshakeManager::new(
        node.veilid.clone(),
        background.clone(),
        our_dht_key.clone(),
    )
    .into_shared();
    HandshakeManager::start_background_task(handshake_manager.clone(), node.clone());

    let walk_task = match main_dht_index {
        Some(public_dht_package) => {
            let init = WalkTaskInit::new(public_dht_package, background.clone())
                .with_handshake(handshake_manager.clone())
                .with_user_storage(auth.clone(), session.clone());

            match WalkTask::spawn(init).await {
                Ok(task) => {
                    let handler = task.established_peer_handler();
                    handshake_manager
                        .lock()
                        .await
                        .set_established_peer_handler(handler);
                    Some(task)
                }
                Err(err) => {
                    println!("Network walker could not start: {err}");
                    None
                }
            }
        }
        None => None,
    };

    let mut current_walk: Option<WalkHandle> = None;

    println!("Main is still running.");
    println!("Starting user controlled loop:");

    loop {
        println!(" ");
        println!("*************************************");
        println!("* N - New DHT                       *");
        println!("* G - Get DHT Data                  *");
        println!("* W - Write to DHT (one subkey)     *");
        println!("* A - Bombard: write ALL subkeys    *");
        println!("* R - Read one owned subkey         *");
        println!("* L - Read aLL owned subkeys        *");
        println!("* E - External DHT: one subkey      *");
        println!("* X - External DHT: read all        *");
        println!("* Y - External DHT: partial parallel*");
        println!("* S - Save DHTs to your account     *");
        println!("* C - Check route manager status    *");
        println!("* D - Debug: show account/DHT info  *");
        println!("* H - Handshake with a peer's DHT   *");
        println!("* K - checK handshake status        *");
        println!("* T - sTart a network walk          *");
        println!("* P - walk Progress                 *");
        println!("* I - Internal node list            *");
        println!("* O - stOp current walk             *");
        println!("* Q - Quit (auto-saves)             *");
        println!("*************************************");
        let choice = read_line("Choice (n/g/w/a/r/l/e/x/s/c/d/h/k/t/p/i/o/q...): ");
        match choice.trim() {
            "n" | "N" => {
                let name = read_line("Name for this DHT: ");
                let subkey_groups = read_subkey_groups();

                let total: u32 = subkey_groups.iter().map(|&n| n as u32).sum();

                println!(
                    "Trying to create DHT '{}' with {} owner group(s), {} subkeys total...",
                    name,
                    subkey_groups.len(),
                    total
                );

                match background.create_dht(name, subkey_groups).await {
                    Ok(index) => println!("Created DHT at index {}", index),
                    Err(err) => println!("Failed to create DHT: {:?}", err),
                }
            }

            "g" | "G" => {
                let index = read_index("Index of DHT to inspect: ");
                match background.get_dht_info(index).await {
                    Some(package) => {
                        let subkey_count: u32 = package
                            .subkey_ranges
                            .iter()
                            .map(|(start, end)| end - start)
                            .sum();

                        println!("DHT name: {}", package.name);
                        println!("Subkey count: {}", subkey_count);
                        println!("Owner (keypair) count: {}", package.keypairs.len());
                        println!("Record key: {}", package.dht_record.key());
                    }
                    None => println!("No DHT package exists at index {}", index),
                }
            }

            "w" | "W" => {
                let index = read_index("Index of DHT to write to: ");
                let location: u32 = loop {
                    let raw = read_line("Subkey location to write to: ");
                    match raw.parse::<u32>() {
                        Ok(n) => break n,
                        _ => println!("Please enter a valid non-negative number."),
                    }
                };

                let data = read_line("Data to write: ");

                match background.write_to_dht(index, location, data.into_bytes()).await {
                    Ok(_) => println!("Write successful"),
                    Err(err) => println!("Write failed: {:?}", err),
                }
            }

            "a" | "A" => {
                let index = read_index("Index of DHT to bombard: ");

                let size: u32 = match background.get_dht_info(index).await {
                    Some(package) => package
                        .subkey_ranges
                        .iter()
                        .map(|(start, end)| end - start)
                        .sum(),
                    None => {
                        println!("No DHT package exists at index {}", index);
                        continue;
                    }
                };

                println!("Firing off {} simultaneous writes to DHT index {}...", size, index);

                for loc in 0..size {
                    let background = background.clone();
                    let payload = format!("bulk-write-{}", loc).into_bytes();

                    tokio::spawn(async move {
                        match background.write_to_dht(index, loc, payload).await {
                            Ok(_) => println!("[bombard] subkey {} write OK", loc),
                            Err(err) => println!("[bombard] subkey {} write FAILED: {:?}", loc, err),
                        }
                    });
                }

                println!("All {} writes dispatched. Watch above for results as they land.", size);
            }

            "r" | "R" => {
                let index = read_index("Index of DHT to read from: ");
                let location: u32 = loop {
                    let raw = read_line("Subkey location to read: ");
                    match raw.parse::<u32>() {
                        Ok(n) => break n,
                        _ => println!("Please enter a valid non-negative number."),
                    }
                };

                match background.read_from_dht(index, location, false).await {
                    Ok(data) => match String::from_utf8(data.clone()) {
                        Ok(text) => println!("Subkey {} -> \"{}\"", location, text),
                        Err(_) => println!("Subkey {} -> {} raw bytes", location, data.len()),
                    },
                    Err(err) => println!("Read failed: {:?}", err),
                }
            }

            "l" | "L" => {
                let index = read_index("Index of DHT to read (all subkeys): ");

                match background.read_all_dht(index, false).await {
                    Ok(results) => {
                        println!("Read {} subkey(s) from DHT index {}:", results.len(), index);
                        for (loc, result) in results {
                            match result {
                                Ok(data) => match String::from_utf8(data.clone()) {
                                    Ok(text) => println!("  [{}] -> \"{}\"", loc, text),
                                    Err(_) => println!("  [{}] -> {} raw bytes", loc, data.len()),
                                },
                                Err(err) => println!("  [{}] -> FAILED: {:?}", loc, err),
                            }
                        }
                    }
                    Err(err) => println!("Batch read failed: {:?}", err),
                }
            }

            "e" | "E" => {
                let record_key_text = read_line("External DHT record key: ");
                let record_key: RecordKey = match record_key_text.trim().parse() {
                    Ok(key) => key,
                    Err(err) => {
                        println!("Invalid DHT record key: {err:?}");
                        continue;
                    }
                };

                let location: u32 = loop {
                    let raw = read_line("Subkey location to read: ");
                    match raw.parse::<u32>() {
                        Ok(n) => break n,
                        _ => println!("Please enter a valid non-negative number."),
                    }
                };

                match background
                    .read_foreign_subkey(record_key, location, true)
                    .await
                {
                    Ok(data) => print_dht_value(location, &data),
                    Err(err) => println!("External read failed: {:?}", err),
                }
            }

            "x" | "X" => {
                let record_key_text = read_line("External DHT record key: ");
                let record_key: RecordKey = match record_key_text.trim().parse() {
                    Ok(key) => key,
                    Err(err) => {
                        println!("Invalid DHT record key: {err:?}");
                        continue;
                    }
                };

                match background.read_all_foreign_dht(record_key, true).await {
                    Ok(results) => {
                        println!("Read {} subkey(s) from external DHT:", results.len());
                        for (location, result) in results {
                            match result {
                                Ok(data) => print_dht_value(location, &data),
                                Err(dht_module::CreateDhtError::NotFound) => {
                                    println!("  [{}] -> <unset>", location)
                                }
                                Err(err) => println!("  [{}] -> FAILED: {:?}", location, err),
                            }
                        }
                    }
                    Err(err) => println!("External batch read failed: {:?}", err),
                }
            }

            "y" | "Y" => {
                let record_key_text = read_line("External DHT record key: ");
                let record_key: RecordKey = match record_key_text.trim().parse() {
                    Ok(key) => key,
                    Err(err) => {
                        println!("Invalid DHT record key: {err:?}");
                        continue;
                    }
                };

                let locations = read_subkey_selection(
                    "Subkeys (examples: 0,1,10,50-75): ",
                );

                match background
                    .read_foreign_subkeys(record_key, locations, true)
                    .await
                {
                    Ok(results) => {
                        println!("Partial parallel read returned {} result(s):", results.len());
                        for (location, result) in results {
                            match result {
                                Ok(data) => print_dht_value(location, &data),
                                Err(dht_module::CreateDhtError::NotFound) => {
                                    println!("  [{}] -> <unset>", location)
                                }
                                Err(err) => println!("  [{}] -> FAILED: {:?}", location, err),
                            }
                        }
                    }
                    Err(err) => println!("External partial read failed: {:?}", err),
                }
            }

            "s" | "S" => {
                let snapshot = background.export_snapshot().await;
                match auth.write_user_encrypted(&session, DHT_SNAPSHOT_KEY, &snapshot) {
                    Ok(()) => println!("Saved {} DHT(s) to your account.", snapshot.len()),
                    Err(err) => println!("Failed to save DHTs: {:?}", err),
                }
            }

            "c" | "C" => {
                let status = route_manager.get_status().await;
                println!("Route manager readiness: {:?}", status.readiness);
                println!("Route manager publish state: {:?}", status.publish_state);
                match status.active_route_id {
                    Some(id) => println!("Active route id: {:?}", id),
                    None => println!("No active route published yet."),
                }
            }

            "d" | "D" => {
                println!("Logged in as: {}", session.username());

                let snapshot = background.export_snapshot().await;
                println!("{} DHT(s) currently tracked in memory:", snapshot.len());
                for (i, record) in snapshot.iter().enumerate() {
                    let subkeys: u32 = record
                        .subkey_ranges
                        .iter()
                        .map(|(start, end)| end - start)
                        .sum();
                    println!(
                        "  [{}] \"{}\" - {} owner group(s), {} subkey(s)",
                        i,
                        record.name,
                        record.keypairs.len(),
                        subkeys
                    );
                }

                match auth.read_user_setup_state(&session) {
                    Ok(state) => {
                        println!("Main DHT setup complete: {}", state.main_dht_setup);
                        match state.main_dht_package_index {
                            Some(index) => println!("Main DHT package index on file: {index}"),
                            None => println!("No main DHT package index saved yet."),
                        }
                    }
                    Err(error) => println!("Error reading user setup state: {error:?}"),
                }
            }

            "h" | "H" => {
                let peer_dht = read_line("Peer's DHT record key (the address they gave you): ");

                if peer_dht.trim().is_empty() {
                    println!("Empty DHT address, nothing to do.");
                    continue;
                }

                if our_dht_key.is_empty() {
                    println!(
                        "Warning: we don't have a route DHT of our own yet, so the peer won't \
                         be able to reply. Continuing anyway..."
                    );
                }

                let mut mgr = handshake_manager.lock().await;
                match mgr.initiate_handshake(peer_dht.trim().to_string()).await {
                    Ok(()) => println!(
                        "Handshake initiated. Use 'k' to check its status, or watch the \
                         [handshake] log lines above as messages come in."
                    ),
                    Err(err) => println!("Failed to initiate handshake: {err}"),
                }
            }

            "k" | "K" => {
                let peer_dht = read_line("Peer's DHT record key to check: ");
                let mgr = handshake_manager.lock().await;

                match mgr.session(peer_dht.trim()) {
                    Some(state) => {
                        println!("Status: {:?}", state.status);
                        println!("Is initiator: {}", state.is_initiator);
                        println!("Encryption mode: {:?}", state.encryption_mode);
                        println!("Retries so far: {}", state.retries);
                    }
                    None => println!("No handshake session on file for that DHT address."),
                }
            }

            "t" | "T" => {
                let Some(walker) = &walk_task else {
                    println!("The network walker is not available.");
                    continue;
                };

                let hops = read_index("How many hops should this walk attempt? ");
                match walker.start_walk(WalkConfig::random(hops)).await {
                    Ok(WalkStartResult::Started(handle)) => {
                        println!("Walk started with {hops} requested hop(s).");
                        current_walk = Some(handle);
                    }
                    Ok(WalkStartResult::AlreadyRunning(handle)) => {
                        println!(
                            "A walk is already running; about {} hop(s) remain.",
                            handle.estimated_hops_remaining()
                        );
                        current_walk = Some(handle);
                    }
                    Err(err) => println!("Could not start walk: {err}"),
                }
            }

            "p" | "P" => match &current_walk {
                Some(handle) => match handle.status() {
                    WalkStatus::Running {
                        requested_hops,
                        completed_hops,
                        current_target,
                    } => {
                        println!("Walk progress: {completed_hops}/{requested_hops}");
                        if let Some(target) = current_target {
                            println!("Currently reading: {target}");
                        }
                    }
                    WalkStatus::Finished(report) => println!("Last walk: {report:?}"),
                    WalkStatus::Failed(message) => println!("Walk failed: {message}"),
                },
                None => println!("No walk has been started during this run."),
            },

            "i" | "I" => {
                let Some(walker) = &walk_task else {
                    println!("The network walker is not available.");
                    continue;
                };

                let list = walker.get_internal_list_copy().await;
                println!("Internal node list contains {} peer(s).", list.len());
                for (index, entry) in list.entries.iter().enumerate().take(50) {
                    println!(
                        "  [{index}] {} | last seen {} | mentioned by {} peer(s)",
                        entry.their_address,
                        entry.last_seen,
                        entry.seen_in.len()
                    );
                }
                if list.len() > 50 {
                    println!("  ...and {} more", list.len() - 50);
                }
            }

            "o" | "O" => match &current_walk {
                Some(handle) if handle.is_active() => {
                    handle.cancel();
                    println!("Walk cancellation requested.");
                }
                _ => println!("No active walk to stop."),
            },

            "q" | "Q" => {
                let snapshot = background.export_snapshot().await;
                if let Err(err) = auth.write_user_encrypted(&session, DHT_SNAPSHOT_KEY, &snapshot) {
                    println!("Warning: failed to save DHTs before quitting: {:?}", err);
                } else {
                    println!("Saved {} DHT(s) to your account.", snapshot.len());
                }

                println!("Shutting down...");
                break;
            }

            _ => println!("Unknown choice. Use n, g, w, a, r, l, e, x, y, s, c, d, h, k, t, p, i, o, or q."),
        }
    }

    if let Some(walker) = &walk_task {
        if let Err(err) = walker.shutdown().await {
            println!("Walker shutdown warning: {err}");
        }
    }

    node.shutdown().await;
    println!("Safely Shut Down");

    Ok(())
}

/// Loops until the user successfully logs in or signs up.
fn login_or_signup(auth: &UserAuth) -> UserSession {
    loop {
        let choice = read_line("Login or Signup? (l/s): ");
        let username = read_line("Username: ");
        let password = read_line("Password: "); // plaintext echo - see note below

        let result = match choice.trim() {
            "l" | "L" => auth.login(&username, &password),
            "s" | "S" => auth.signup(&username, &password),
            _ => {
                println!("Please enter l or s.");
                continue;
            }
        };

        match result {
            Ok(session) => return session,
            Err(AuthError::UserNotFound) => println!("No account with that username."),
            Err(AuthError::UserAlreadyExists) => println!("That username is already taken."),
            Err(AuthError::WrongPassword) => println!("Wrong password."),
            Err(AuthError::InvalidUsername) => {
                println!("Usernames may only contain letters, numbers, '_' and '-'.")
            }
            Err(err) => println!("Auth error: {:?}", err),
        }
    }
}

fn read_index(prompt: &str) -> usize {
    loop {
        let raw = read_line(prompt);
        match raw.parse::<usize>() {
            Ok(n) => return n,
            _ => println!("Please enter a valid non-negative number."),
        }
    }
}

/// Prompts the user to build up the list of owner groups for a new DHT.
fn read_subkey_groups() -> Vec<u16> {
    let mut groups: Vec<u16> = Vec::new();

    loop {
        let size: u16 = loop {
            let raw = read_line(&format!(
                "Subkeys for owner group #{} (1-250): ",
                groups.len() + 1
            ));
            match raw.parse::<u16>() {
                Ok(n) if n >= 1 && n <= 250 => break n,
                _ => println!("Please enter a number between 1 and 250."),
            }
        };

        groups.push(size);

        if groups.len() >= 250 {
            println!("Reached the maximum of 250 owner groups.");
            break;
        }

        loop {
            let more = read_line("Add another owner group? (y/n): ");
            match more.trim() {
                "y" | "Y" => break,
                "n" | "N" => return groups,
                _ => println!("Please enter y or n."),
            }
        }
    }

    groups
}

fn read_subkey_selection(prompt: &str) -> Vec<u32> {
    loop {
        let raw = read_line(prompt);
        let mut locations = Vec::new();
        let mut valid = true;

        for part in raw.split(',').map(str::trim).filter(|part| !part.is_empty()) {
            if let Some((start, end)) = part.split_once('-') {
                match (start.trim().parse::<u32>(), end.trim().parse::<u32>()) {
                    (Ok(start), Ok(end)) if start <= end => locations.extend(start..=end),
                    _ => {
                        valid = false;
                        break;
                    }
                }
            } else {
                match part.parse::<u32>() {
                    Ok(location) => locations.push(location),
                    Err(_) => {
                        valid = false;
                        break;
                    }
                }
            }
        }

        if valid && !locations.is_empty() {
            locations.sort_unstable();
            locations.dedup();
            return locations;
        }

        println!("Enter comma-separated subkeys and/or inclusive ranges, such as 0,1,10,50-75.");
    }
}

fn read_line(prompt: &str) -> String {
    print!("{prompt}");
    io::stdout().flush().ok();
    let mut buf = String::new();
    io::stdin()
        .read_line(&mut buf)
        .expect("Failed to read line");
    buf.trim().to_owned()
}

fn print_dht_value(location: u32, data: &[u8]) {
    match std::str::from_utf8(data) {
        Ok(text) => println!("  [{}] -> \"{}\"", location, text),
        Err(_) => println!("  [{}] -> {} raw bytes: {:?}", location, data.len(), data),
    }
}
