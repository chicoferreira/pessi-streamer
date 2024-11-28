use aes_gcm::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    Aes256Gcm, Nonce, Key
};

const PASSWORD: &str = "Z8J3SZ894R!88^3MjghwF&G$w5ld%d#A"; // Needs to be 32 bytes

pub fn encrypt(data: &[u8]) -> Vec<u8> {
    let key = derive_key(PASSWORD);
    let cipher = Aes256Gcm::new(&key);

    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
    let encrypted = cipher.encrypt(&nonce, data.as_ref()).unwrap();

    let mut result = Vec::new();
    result.extend_from_slice(&nonce);
    result.extend_from_slice(&encrypted);
    result
}

pub fn decrypt(encrypted_data: &[u8]) -> Vec<u8> {
    let key = derive_key(PASSWORD); 
    let cipher = Aes256Gcm::new(&key);

    let nonce = Nonce::from_slice(&encrypted_data[0..12]);
    let data = &encrypted_data[12..];

    cipher.decrypt(nonce, data).unwrap()
}

fn derive_key(password: &str) -> Key<Aes256Gcm> {
    *Key::<Aes256Gcm>::from_slice(password.as_bytes())
}
