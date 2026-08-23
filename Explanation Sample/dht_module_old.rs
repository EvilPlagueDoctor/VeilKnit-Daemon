use std::sync::Arc;
use std::io::{self, Write};
use tokio::sync::{mpsc, oneshot};
use veilid_core::*;


// ===========================================================================
//
// REMEMBER:
// It's on YOU to save any of this data for a diffrent session, this just deals with the DHT stuff.
//
// ===========================================================================


// the error list for when creating a DHT
#[derive(Debug)]
pub enum CreateDhtError {
    ZeroSubkeys,
    TooManySubkeys,
    MalformedName,
    VeilidError(String),	// this allows us a place to cram in veilid's errors as it sees fit.
}

/// Commands that the background task understands.
enum DHTCommand {
    Add {
        a: i32,
        b: i32,
        reply: oneshot::Sender<i32>,
    },

    Count {
        value: i32,
    },

    CreateDHT {
	name: String,	// name of the DHT (example 'Main' might be my central DHT and 'Mailbox' might be the DHT that holds my mailbox information, etc.
	size: u16,	// how big the DHT will be
	reply: oneshot::Sender<Result<usize, CreateDhtError>>	// responds with the package location (or a specific error if it's a failure)
    },

    GetDhtInfo {
        index: usize,	// what DHT package you want the info on.
        reply: oneshot::Sender<Option<RecordKeyPackage>>,
    },

    WriteToDHT {
	dht_package: usize,
        location: u32,	// what line in the DHT to try and write to.
	data: Vec<u8>,
	reply: oneshot::Sender<Result<usize, CreateDhtError>>	// responds 0 on success, or a specific error if it's a failure
    },

    // maybe have a check to see if a DHT is routable.
    // have a way to load up the information of a DHT (and make it accessible)
    // have a way to read the DHT (either in full, or in part)
}

// ========================================================================================
// RecordKeyPackage, stores the record key, the sub keys, and the 'laymans name' used for keeping track of which of these packages belong to whom 
// (internally it's position, but depending on creation time, those positions can change)
// ========================================================================================
#[derive(Debug, Clone)]
pub struct RecordKeyPackage {
    pub dht_record: DHTRecordDescriptor,	// contains the Record Key, but also lots of other goodies too!
    pub keypairs: Vec<KeyPair>,	// The list of key pairs (position in the vec corrolates to position in the DHT)
    pub name: String,		// laymans name for the DHT
    pub our_id: MemberId,	// the ID we used to create this.
}

/// Public handle that other code uses.
#[derive(Clone)]
pub struct DHTModule {
    sender: mpsc::Sender<DHTCommand>,	// basically stores the 'command' that the user sent and stores a que if multiple commands get sent at the same time.
}

impl DHTModule {
    /// Starts the background task. (DHTModule)
    pub fn new(veilid_imported: Arc<VeilidAPI>) -> Self {
        let (sender, mut rx) = mpsc::channel::<DHTCommand>(32);

        tokio::spawn(async move {
            println!("DHTModule task started.");

	    // STORED VARIABLES
	    let mut our_recordkeys: Vec<RecordKeyPackage> = Vec::new();		// Holds a list of DHT's we own.
	    let veilid = veilid_imported;

            while let Some(command) = rx.recv().await {
                match command {
                    DHTCommand::Add { a, b, reply } => {
                        println!("Adding {} + {}", a, b);

                        let result = a + b;

                        let _ = reply.send(result);
                    }

                    DHTCommand::Count { value } => {
                        println!("Counting...");

                        for i in 1..=value {
                            println!("{} + {} = {}", value, i, value + i);
                        }

                        println!("Finished counting.");
                    }

		//========================================================================================================
		// Create a DHT
		//========================================================================================================

		    /// Create a new DHT record and register its owner keypair with the store.
		    DHTCommand::CreateDHT { name, size, reply } => {
			println!("Creating DHT");

			// Do some basic error checking... except now the entire freaking thing is going to be wrapped in an error checking thing..woo..
		        let result: Result<usize, CreateDhtError> = async {
			if size == 0 {
		            return Err(CreateDhtError::ZeroSubkeys);
		        } 
			if size > 250 {
		            return Err(CreateDhtError::TooManySubkeys);
		        }
			if name.trim().is_empty() {
		            return Err(CreateDhtError::MalformedName);
		        } 
		            

			    // create a key pair to act as our main key pair.
			    let owner_kp = Crypto::generate_keypair(CRYPTO_KIND_VLD0).map_err(|e| CreateDhtError::VeilidError(e.to_string()))?;
			    // take a clone of that, and split it.
			    let (owner_pub, _owner_secret) = owner_kp.clone().into_split();
			    // create an ID based off that key:
			    let owner_id = veilid.generate_member_id(&owner_pub).map_err(|e| CreateDhtError::VeilidError(e.to_string()))?;
			    let bare_id   = owner_id.clone().into_value();
			    // set our options. (not actually needed, the defaults are fine)
		            let owner_opts = SetDHTValueOptions {
		                writer:        Some(owner_kp.clone()),
		                allow_offline: None,
		            };
				
			    // create our schema:
			    let smpl_member = DHTSchemaSMPLMember {
		                m_key: bare_id,
		                m_cnt: size,
		            };

			    // load up our stuff to a vec (I forget how it's ordered, so I'm not quite sure how it plays with our vec with the owner key pair in there)
			    let schema = DHTSchema::smpl(0, vec![smpl_member]).map_err(|e| CreateDhtError::VeilidError(e.to_string()))?;
				
			    schema.validate().map_err(|e| CreateDhtError::VeilidError(e.to_string()))?;

			    // Routing context because we have to deal with potential errors higher up the chain, blah.
			    let veilid_rc = veilid.routing_context().map_err(|e| CreateDhtError::VeilidError(e.to_string()))?;

			    // upload the DHT
		            let record_desc = veilid_rc
		                    .create_dht_record(CRYPTO_KIND_VLD0, schema, None)
		                    .await.map_err(|e| CreateDhtError::VeilidError(e.to_string()))?;

let record_key = record_desc.key();
			    // wait for it to be routable

    println!("\nWaiting for DHT record to become routable...");
    loop {
        match veilid_rc
            .inspect_dht_record(record_key.clone(), None, DHTReportScope::SyncGet)
            .await
        {
            Ok(_)                                => break,
            Err(VeilidAPIError::TryAgain { .. }) => {
                print!(".");
                io::stdout().flush().ok();
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }
            Err(e) => return Err(CreateDhtError::VeilidError(e.to_string())),
        }
    }
    println!("\nRecord is ready.\n");

			    // set everything up to push to our package.
			    let package = RecordKeyPackage {
			        dht_record: record_desc,
			        keypairs: vec![owner_kp],
			        name,
			        our_id: owner_id,
			    };

			    // get the index length (how many 'DHT's we already own)
			    let index = our_recordkeys.len();
		            // push RecordKeyPackage into owned_recordkeys
			    our_recordkeys.push(package);

		            // return its index
		            Ok(index)
		        
		    }.await; // the end of the dumb error thingy
		    let _ = reply.send(result);
		    }

		//=========================================================================================================
		// Get the info on a DHT package that's been created by us. (for things like saving)
		//=========================================================================================================

		    DHTCommand::GetDhtInfo { index, reply } => {
		        let result = our_recordkeys.get(index).cloned();
		        let _ = reply.send(result);
		    }

		//=========================================================================================================
		// Write to the DHT (each write get's it's own task spawned)
		//=========================================================================================================

		    DHTCommand::WriteToDHT {dht_package, location, data, reply} => {
		        let package = our_recordkeys.get(dht_package).cloned();
		        let veilid = veilid.clone();

    		    tokio::spawn(async move {
        		let result: Result<usize, CreateDhtError> = async {
            		    let package = package.ok_or(CreateDhtError::MalformedName)?;

		            let veilid_rc = veilid.routing_context().map_err(|e| CreateDhtError::VeilidError(e.to_string()))?;

            		    let record_key = package.dht_record.key().clone();

		            // grab the writer keypair for this record
		            let writer_kp = package.keypairs.get(0)
		                .cloned()
		                .ok_or(CreateDhtError::MalformedName)?;

		            let write_opts = SetDHTValueOptions {
		                writer: Some(writer_kp),
		                allow_offline: None,
		            };

            		    veilid_rc.set_dht_value(record_key, location, data, Some(write_opts)).await.map_err(|e| CreateDhtError::VeilidError(e.to_string()))?;

            		    Ok(0)
        		    }.await;

        		    let _ = reply.send(result);
		        });
		    }

                }
            }

            println!("Background task shutting down.");

        });

        Self { sender }
    }

    /// Sends an Add command and waits for the answer.
    pub async fn add(&self, a: i32, b: i32) -> i32 {
        let (reply_sender, reply_rx) = oneshot::channel();

        self.sender
            .send(DHTCommand::Add {
                a,
                b,
                reply: reply_sender,
            })
            .await
            .unwrap();

        reply_rx.await.unwrap()
    }

    /// Sends a Count command.
    pub async fn count(&self, value: i32) {
        self.sender
            .send(DHTCommand::Count { value })
            .await
            .unwrap();
    }

    pub async fn create_dht(
        &self,
        name: String,
        size: u16,
    ) -> Result<usize, CreateDhtError> {
        let (reply_sender, reply_rx) = oneshot::channel();

        self.sender
            .send(DHTCommand::CreateDHT {
                name,
                size,
                reply: reply_sender,
            })
            .await
            .unwrap();

        reply_rx.await.unwrap()
    }

    pub async fn get_dht_info(&self, index: usize) -> Option<RecordKeyPackage> {
        let (reply_sender, reply_rx) = oneshot::channel();

        self.sender
            .send(DHTCommand::GetDhtInfo {
                index,
                reply: reply_sender,
            })
            .await
            .unwrap();

        reply_rx.await.unwrap()
    }

    pub async fn write_to_dht(
        &self,
        dht_package: usize,
        location: u32,
        data: Vec<u8>,
    ) -> Result<usize, CreateDhtError> {
        let (reply_sender, reply_rx) = oneshot::channel();

        self.sender
            .send(DHTCommand::WriteToDHT {
                dht_package,
                location,
                data,
                reply: reply_sender,
            })
            .await
            .unwrap();

        reply_rx.await.unwrap()
    }

} // end of the IMPL dht module.