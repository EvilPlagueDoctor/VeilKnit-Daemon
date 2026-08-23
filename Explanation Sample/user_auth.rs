use aes_gcm::{
    aead::{Aead, OsRng, rand_core::RngCore},
    Aes256Gcm, KeyInit, Nonce,
};
use argon2::{
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};
use zeroize::Zeroizing;

/// Encrypted user-store key for account-level setup state.
pub const USER_SETUP_STATE_KEY: &str = "user_setup_state";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UserSetupState {
    /// True only after the main DHT has been created, initialized, saved,
    /// and handed to the route manager.
    pub main_dht_setup: bool,

    /// DHTModule package index assigned to the main DHT.
    pub main_dht_package_index: Option<usize>,
}

#[derive(Debug)]
pub enum AuthError {
    UserAlreadyExists,
    UserNotFound,
    WrongPassword,
    InvalidUsername,
    Io(String),
    Crypto(String),
    Serde(String),
}

impl From<std::io::Error> for AuthError {
    fn from(e: std::io::Error) -> Self {
        AuthError::Io(e.to_string())
    }
}

impl std::fmt::Display for AuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuthError::UserAlreadyExists => write!(f, "user already exists"),
            AuthError::UserNotFound => write!(f, "user not found"),
            AuthError::WrongPassword => write!(f, "wrong password"),
            AuthError::InvalidUsername => write!(f, "invalid username"),
            AuthError::Io(msg) => write!(f, "io error: {msg}"),
            AuthError::Crypto(msg) => write!(f, "crypto error: {msg}"),
            AuthError::Serde(msg) => write!(f, "serialization error: {msg}"),
        }
    }
}

impl std::error::Error for AuthError {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserInfo {
    pub username: String,
    pub display_name: String,
    pub created_at: u64,
}

#[derive(Debug, Serialize, Deserialize)]
struct AccountFile {
    password_hash: String,
    crypto_salt: Vec<u8>,
}

pub struct UserSession {
    user: UserInfo,
    user_dir: PathBuf,
    key: Zeroizing<[u8; 32]>,
}

impl UserSession {
    pub fn user(&self) -> &UserInfo {
        &self.user
    }

    pub fn username(&self) -> &str {
        &self.user.username
    }

    fn user_dir(&self) -> &Path {
        &self.user_dir
    }

    fn encrypt(&self, plaintext: &[u8]) -> Result<Vec<u8>, AuthError> {
        let cipher = Aes256Gcm::new_from_slice(self.key.as_ref())
            .map_err(|e| AuthError::Crypto(e.to_string()))?;

        let mut nonce_bytes = [0u8; 12];
        OsRng.fill_bytes(&mut nonce_bytes);

        let nonce = Nonce::from_slice(&nonce_bytes);

        let mut ciphertext = cipher
            .encrypt(nonce, plaintext)
            .map_err(|e| AuthError::Crypto(e.to_string()))?;

        let mut out = Vec::with_capacity(12 + ciphertext.len());
        out.extend_from_slice(&nonce_bytes);
        out.append(&mut ciphertext);

        Ok(out)
    }

    fn decrypt(&self, encrypted: &[u8]) -> Result<Vec<u8>, AuthError> {
        if encrypted.len() < 12 {
            return Err(AuthError::Crypto("encrypted data too short".into()));
        }

        let cipher = Aes256Gcm::new_from_slice(self.key.as_ref())
            .map_err(|e| AuthError::Crypto(e.to_string()))?;

        let nonce = Nonce::from_slice(&encrypted[..12]);
        let ciphertext = &encrypted[12..];

        cipher
            .decrypt(nonce, ciphertext)
            .map_err(|_| AuthError::WrongPassword)
    }
}

pub struct UserAuth {
    root_dir: PathBuf,
}

impl UserAuth {
    pub fn new(root_dir: impl Into<PathBuf>) -> Result<Self, AuthError> {
        let root_dir = root_dir.into();

        fs::create_dir_all(root_dir.join("users"))?;
        fs::create_dir_all(root_dir.join("global"))?;

        Ok(Self { root_dir })
    }

    pub fn signup(&self, username: &str, password: &str) -> Result<UserSession, AuthError> {
        validate_username(username)?;

        let user_dir = self.user_dir(username);
        let account_path = user_dir.join("account.json");
        let profile_path = user_dir.join("user_profile.bin");

        if account_path.exists() {
            return Err(AuthError::UserAlreadyExists);
        }

        fs::create_dir_all(&user_dir)?;

        let password_salt = SaltString::generate(&mut OsRng);

        let password_hash = Argon2::default()
            .hash_password(password.as_bytes(), &password_salt)
            .map_err(|e| AuthError::Crypto(e.to_string()))?
            .to_string();

        let mut crypto_salt = vec![0u8; 16];
        OsRng.fill_bytes(&mut crypto_salt);

        let account = AccountFile {
            password_hash,
            crypto_salt: crypto_salt.clone(),
        };

        let account_bytes = serde_json::to_vec_pretty(&account)
            .map_err(|e| AuthError::Serde(e.to_string()))?;

        atomic_write(&account_path, &account_bytes)?;

        let key = derive_key(password, &crypto_salt)?;

        let session = UserSession {
            user: UserInfo {
                username: username.to_string(),
                display_name: username.to_string(),
                created_at: current_timestamp(),
            },
            user_dir,
            key,
        };

        let profile_bytes = serde_json::to_vec_pretty(session.user())
            .map_err(|e| AuthError::Serde(e.to_string()))?;

        let encrypted_profile = session.encrypt(&profile_bytes)?;
        atomic_write(&profile_path, &encrypted_profile)?;

        // A new account starts without a configured main DHT. The user_dht
        // module changes this only after the complete setup workflow succeeds.
        self.write_user_setup_state(&session, &UserSetupState::default())?;

        Ok(session)
    }

    pub fn login(&self, username: &str, password: &str) -> Result<UserSession, AuthError> {
        validate_username(username)?;

        let user_dir = self.user_dir(username);
        let account_path = user_dir.join("account.json");
        let profile_path = user_dir.join("user_profile.bin");

        if !account_path.exists() {
            return Err(AuthError::UserNotFound);
        }

        let account_bytes = fs::read(&account_path)?;
        let account: AccountFile = serde_json::from_slice(&account_bytes)
            .map_err(|e| AuthError::Serde(e.to_string()))?;

        let parsed_hash = PasswordHash::new(&account.password_hash)
            .map_err(|e| AuthError::Crypto(e.to_string()))?;

        Argon2::default()
            .verify_password(password.as_bytes(), &parsed_hash)
            .map_err(|_| AuthError::WrongPassword)?;

        let key = derive_key(password, &account.crypto_salt)?;

        let temp_session = UserSession {
            user: UserInfo {
                username: username.to_string(),
                display_name: username.to_string(),
                created_at: 0,
            },
            user_dir,
            key,
        };

        let encrypted_profile = fs::read(&profile_path)?;
        let profile_bytes = temp_session.decrypt(&encrypted_profile)?;

        let user: UserInfo = serde_json::from_slice(&profile_bytes)
            .map_err(|e| AuthError::Serde(e.to_string()))?;

        Ok(UserSession {
            user,
            user_dir: temp_session.user_dir,
            key: temp_session.key,
        })
    }

    pub fn write_user_encrypted<T: Serialize>(
        &self,
        session: &UserSession,
        key: &str,
        value: &T,
    ) -> Result<(), AuthError> {
        let dir = session.user_dir().join("store");
        fs::create_dir_all(&dir)?;

        let path = dir.join(format!("{}.bin", clean_key(key)));

        let bytes = serde_json::to_vec_pretty(value)
            .map_err(|e| AuthError::Serde(e.to_string()))?;

        let encrypted = session.encrypt(&bytes)?;

        atomic_write(&path, &encrypted)
    }

    pub fn read_user_encrypted<T: DeserializeOwned>(
        &self,
        session: &UserSession,
        key: &str,
    ) -> Result<Option<T>, AuthError> {
        let path = session
            .user_dir()
            .join("store")
            .join(format!("{}.bin", clean_key(key)));

        if !path.exists() {
            return Ok(None);
        }

        let encrypted = fs::read(path)?;
        let bytes = session.decrypt(&encrypted)?;

        let value = serde_json::from_slice(&bytes)
            .map_err(|e| AuthError::Serde(e.to_string()))?;

        Ok(Some(value))
    }

    pub fn read_user_setup_state(
        &self,
        session: &UserSession,
    ) -> Result<UserSetupState, AuthError> {
        Ok(self
            .read_user_encrypted::<UserSetupState>(session, USER_SETUP_STATE_KEY)?
            .unwrap_or_default())
    }

    pub fn write_user_setup_state(
        &self,
        session: &UserSession,
        state: &UserSetupState,
    ) -> Result<(), AuthError> {
        self.write_user_encrypted(session, USER_SETUP_STATE_KEY, state)
    }

    pub fn write_global<T: Serialize>(&self, key: &str, value: &T) -> Result<(), AuthError> {
        let path = self
            .root_dir
            .join("global")
            .join(format!("{}.json", clean_key(key)));

        let bytes = serde_json::to_vec_pretty(value)
            .map_err(|e| AuthError::Serde(e.to_string()))?;

        atomic_write(&path, &bytes)
    }

    pub fn read_global<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>, AuthError> {
        let path = self
            .root_dir
            .join("global")
            .join(format!("{}.json", clean_key(key)));

        if !path.exists() {
            return Ok(None);
        }

        let bytes = fs::read(path)?;

        let value = serde_json::from_slice(&bytes)
            .map_err(|e| AuthError::Serde(e.to_string()))?;

        Ok(Some(value))
    }

    fn user_dir(&self, username: &str) -> PathBuf {
        let username_hash = blake3::hash(username.as_bytes()).to_hex().to_string();
        self.root_dir.join("users").join(username_hash)
    }
}

fn derive_key(password: &str, salt: &[u8]) -> Result<Zeroizing<[u8; 32]>, AuthError> {
    let mut key = Zeroizing::new([0u8; 32]);

    Argon2::default()
        .hash_password_into(password.as_bytes(), salt, key.as_mut())
        .map_err(|e| AuthError::Crypto(e.to_string()))?;

    Ok(key)
}

fn validate_username(username: &str) -> Result<(), AuthError> {
    let valid = !username.is_empty()
        && username
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');

    if valid {
        Ok(())
    } else {
        Err(AuthError::InvalidUsername)
    }
}

fn clean_key(key: &str) -> String {
    key.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), AuthError> {
    let tmp = path.with_extension("tmp");

    fs::write(&tmp, bytes)?;
    fs::rename(tmp, path)?;

    Ok(())
}

fn current_timestamp() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};

    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}