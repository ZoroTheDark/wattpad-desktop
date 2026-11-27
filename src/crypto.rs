// src/crypto.rs
use chacha20poly1305::{
    aead::{Aead, KeyInit},
    ChaCha20Poly1305, Nonce,
};
use rand::RngCore;
use sha2::{Digest, Sha256};

pub const SALT_LEN: usize = 16;
pub const NONCE_LEN: usize = 12;

/// Genera un salt aleatorio de 16 bytes para la sesión actual.
pub fn generar_salt() -> [u8; SALT_LEN] {
    let mut salt = [0u8; SALT_LEN];
    rand::thread_rng().fill_bytes(&mut salt);
    salt
}

/// Deriva una clave de 32 bytes combinando `username`, `password` y `salt` mediante SHA-256.
pub fn derivar_clave(username: &str, password: &str, salt: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(username.as_bytes());
    hasher.update(password.as_bytes());
    hasher.update(salt);
    hasher.finalize().into()
}

/// Encripta los bytes de un EPUB usando ChaCha20-Poly1305 y devuelve `(datos_cifrados, nonce)`.
pub fn encriptar_epub(datos: &[u8], clave: &[u8; 32]) -> Result<(Vec<u8>, Vec<u8>), String> {
    let mut nonce_bytes = [0u8; NONCE_LEN];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);

    let cipher = ChaCha20Poly1305::new(clave.into());
    let nonce = Nonce::from_slice(&nonce_bytes);
    let cifrado = cipher
        .encrypt(nonce, datos)
        .map_err(|e| format!("Error al encriptar EPUB: {}", e))?;

    Ok((cifrado, nonce_bytes.to_vec()))
}

/// Desencripta los bytes de un EPUB usando ChaCha20-Poly1305 y el `nonce` de 12 bytes.
pub fn desencriptar_epub(
    datos_cifrados: &[u8],
    nonce: &[u8],
    clave: &[u8; 32],
) -> Result<Vec<u8>, String> {
    if nonce.len() != NONCE_LEN {
        return Err(format!(
            "Nonce inválido: se esperaban {} bytes y se recibieron {}",
            NONCE_LEN,
            nonce.len()
        ));
    }

    let cipher = ChaCha20Poly1305::new(clave.into());
    let nonce = Nonce::from_slice(nonce);
    cipher
        .decrypt(nonce, datos_cifrados)
        .map_err(|e| format!("Error al desencriptar EPUB: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encripta_y_desencripta_epub_con_clave_derivada() {
        let salt = generar_salt();
        let clave = derivar_clave("usuario_demo", "secreto_123", &salt);
        let original = b"Contenido de prueba del EPUB de Wattpad";

        let (cifrado, nonce) = encriptar_epub(original, &clave).unwrap();
        assert_ne!(cifrado, original);
        assert_eq!(nonce.len(), NONCE_LEN);

        let descifrado = desencriptar_epub(&cifrado, &nonce, &clave).unwrap();
        assert_eq!(descifrado, original);
    }

    #[test]
    fn falla_desencriptar_con_clave_incorrecta() {
        let salt = generar_salt();
        let clave_ok = derivar_clave("usuario", "clave_correcta", &salt);
        let clave_mal = derivar_clave("usuario", "clave_incorrecta", &salt);

        let (cifrado, nonce) = encriptar_epub(b"datos privados", &clave_ok).unwrap();
        assert!(desencriptar_epub(&cifrado, &nonce, &clave_mal).is_err());
    }
}
