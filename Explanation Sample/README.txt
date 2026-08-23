Network Walk 2 - main DHT ownership cleanup

Changed files:
- dht_module.rs
  * Removed name-based main/public DHT initialization policy.
  * Empty writes and the text "null" are normalized to NULL_DHT_VALUE (b"0").
- user_auth.rs
  * Added persisted UserSetupState.
  * New accounts begin with main_dht_setup = false.
- user_dht.rs
  * New application-policy module.
  * Creates the 251-subkey main DHT.
  * Initializes every subkey concurrently through normal DHTModule::write_to_dht calls.
  * Saves the DHT snapshot, connects RouteManager, then marks setup complete.
- main.rs
  * Delegates main-DHT loading/creation to user_dht.
  * Removed the old route-DHT constants and setup helper.

The other .rs files are included unchanged so the folder can replace the corresponding src files directly.

Note: This environment did not contain cargo/rustc/rustfmt, so I could validate the edits structurally but could not run cargo check here.
