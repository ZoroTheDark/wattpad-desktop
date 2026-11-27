// src/sesion.rs
use cookie_store::CookieStore;
use reqwest_cookie_store::CookieStoreMutex;
use serde::Deserialize;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use wp_mini::WattpadClient;

pub(crate) const USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/125.0.0.0 Safari/537.36";

#[derive(Debug, Clone)]
pub struct SessionInfo {
    pub username: String,
}

#[derive(Debug, Deserialize)]
struct SessionApiUser {
    username: Option<String>,
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SessionApiResponse {
    token: Option<String>,
    user: Option<SessionApiUser>,
}

#[derive(Debug, Deserialize)]
struct CurrentUserResponse {
    username: Option<String>,
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WattpadApiError {
    error_code: Option<i64>,
    message: Option<String>,
}

fn ruta_sesion() -> PathBuf {
    if let Ok(xdg_data) = std::env::var("XDG_DATA_HOME") {
        if !xdg_data.trim().is_empty() {
            return PathBuf::from(xdg_data)
                .join("mi-cliente-wattpad")
                .join("wattpad_sesion.json");
        }
    }

    if let Ok(home) = std::env::var("HOME") {
        if !home.trim().is_empty() {
            return PathBuf::from(home)
                .join(".local")
                .join("share")
                .join("mi-cliente-wattpad")
                .join("wattpad_sesion.json");
        }
    }

    PathBuf::from("wattpad_sesion.json")
}

/// Devuelve `(wp_client, reqwest_client, cookie_store, tiene_token_en_disco)`.
pub fn cargar_cliente() -> (
    WattpadClient,
    reqwest::Client,
    Arc<CookieStoreMutex>,
    bool,
) {
    let ruta = ruta_sesion();
    let cookie_store = if let Ok(file) = std::fs::File::open(&ruta) {
        let reader = std::io::BufReader::new(file);
        cookie_store::serde::json::load(reader).unwrap_or_else(|_| CookieStore::new())
    } else {
        CookieStore::new()
    };

    // Comprobamos que exista una cookie de sesión `token` no expirada, no solo que el archivo exista.
    let tiene_sesion = cookie_store
        .iter_unexpired()
        .any(|c| c.name() == "token" && !c.value().is_empty());

    let cookie_store_arc = Arc::new(CookieStoreMutex::new(cookie_store));

    // Construimos un reqwest::Client con User-Agent real y el cookie store compartido.
    // Nota: wp-mini ignora .user_agent() cuando se usa .reqwest_client(...), por eso se configura aquí.
    let reqwest_client = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(Duration::from_secs(20))
        .cookie_provider(cookie_store_arc.clone())
        .build()
        .expect("No se pudo construir el cliente reqwest");

    let wp_client = crate::api::crear_wattpad_client(reqwest_client.clone());

    (wp_client, reqwest_client, cookie_store_arc, tiene_sesion)
}

/// Guarda el cookie store en disco con permisos privados en Linux (`0600`).
pub fn guardar_sesion(cookie_store_arc: &Arc<CookieStoreMutex>) {
    let ruta = ruta_sesion();
    if let Some(parent) = ruta.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            eprintln!("Error al crear el directorio de sesión: {}", e);
            return;
        }
    }

    let store = cookie_store_arc.lock().unwrap();
    let file = match std::fs::File::create(&ruta) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("Error al crear el archivo de sesión: {}", e);
            return;
        }
    };

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = file.set_permissions(std::fs::Permissions::from_mode(0o600));
    }

    let mut writer = std::io::BufWriter::new(file);
    if let Err(e) = cookie_store::serde::json::save(&store, &mut writer) {
        eprintln!("Error al guardar las cookies: {}", e);
    } else {
        println!("Sesión guardada en {}", ruta.display());
    }
}

/// Inicia sesión usando el endpoint JSON oficial `/v4/sessions` de Wattpad
/// e inyecta la cookie `token` en `.wattpad.com` para que `wp-mini` y `reqwest` compartan la sesión.
pub async fn iniciar_sesion(
    http: &reqwest::Client,
    cookie_store_arc: &Arc<CookieStoreMutex>,
    username: &str,
    password: &str,
) -> Result<SessionInfo, String> {
    let user = username.trim();
    if user.is_empty() || password.is_empty() {
        return Err("Ingresa tu usuario (o correo) y contraseña.".to_string());
    }

    let params = [
        ("type", "wattpad"),
        ("username", user),
        ("password", password),
    ];

    let response = http
        .post("https://api.wattpad.com/v4/sessions")
        .header("Accept", "application/json")
        .form(&params)
        .send()
        .await
        .map_err(|e| format!("Error de red al conectar con Wattpad: {}", e))?;

    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|e| format!("No se pudo leer la respuesta de Wattpad: {}", e))?;

    if !status.is_success() {
        return Err(parsear_error_wattpad(status.as_u16(), &body));
    }

    let parsed: SessionApiResponse = serde_json::from_str(&body)
        .map_err(|e| format!("Respuesta inesperada del servidor ({}): {}", status, e))?;

    let token = parsed
        .token
        .filter(|t| !t.is_empty())
        .or_else(|| {
            let store = cookie_store_arc.lock().unwrap();
            let token = store
                .iter_unexpired()
                .find(|c| c.name() == "token")
                .map(|c| c.value().to_string());
            token
        })
        .ok_or_else(|| {
            "Wattpad respondió sin token de sesión. Es posible que la cuenta requiera verificación adicional.".to_string()
        })?;

    // Guardamos el token como cookie persistente en .wattpad.com para todos los endpoints web/v3/apiv2.
    {
        let wp_url = reqwest::Url::parse("https://www.wattpad.com/")
            .expect("URL base de Wattpad válida");
        let cookie_header = format!(
            "token={}; Domain=.wattpad.com; Path=/; Max-Age=2592000; Secure; HttpOnly",
            token
        );
        let mut store = cookie_store_arc.lock().unwrap();
        if let Err(e) = store.parse(&cookie_header, &wp_url) {
            eprintln!("Advertencia al registrar cookie token: {:?}", e);
        }
    }

    guardar_sesion(cookie_store_arc);

    let resolved_username = parsed
        .user
        .and_then(|u| u.username.or(u.name))
        .unwrap_or_else(|| user.to_string());

    Ok(SessionInfo {
        username: resolved_username,
    })
}

/// Comprueba si las cookies guardadas corresponden a una sesión activa en Wattpad.
pub async fn verificar_sesion(http: &reqwest::Client) -> Result<String, String> {
    let response = http
        .get("https://www.wattpad.com/api/v3/internal/current_user")
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| format!("No se pudo verificar la sesión: {}", e))?;

    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|e| format!("Error leyendo verificación de sesión: {}", e))?;

    if !status.is_success() {
        return Err(parsear_error_wattpad(status.as_u16(), &body));
    }

    let user: CurrentUserResponse = serde_json::from_str(&body)
        .map_err(|e| format!("No se pudo leer el usuario actual: {}", e))?;

    user.username
        .or(user.name)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "La sesión guardada no contiene un usuario válido.".to_string())
}

/// Cierra la sesión actual, limpia las cookies y elimina el archivo guardado.
pub async fn cerrar_sesion(
    http: &reqwest::Client,
    cookie_store_arc: &Arc<CookieStoreMutex>,
) {
    let _ = http.get("https://www.wattpad.com/logout").send().await;
    {
        let mut store = cookie_store_arc.lock().unwrap();
        *store = CookieStore::new();
    }
    let _ = std::fs::remove_file(ruta_sesion());
}

pub(crate) fn parsear_error_wattpad(status: u16, body: &str) -> String {
    if let Ok(err) = serde_json::from_str::<WattpadApiError>(body) {
        return match err.error_code {
            Some(1029) => "Usuario o contraseña incorrectos (código 1029).".to_string(),
            Some(1018) => "La sesión expiró o no has iniciado sesión (código 1018).".to_string(),
            Some(code) => {
                let msg = err.message.unwrap_or_else(|| "Error de Wattpad".to_string());
                format!("{} (código {})", msg, code)
            }
            None => err
                .message
                .unwrap_or_else(|| format!("Error HTTP {} de Wattpad.", status)),
        };
    }

    format!("Error HTTP {} al comunicar con Wattpad.", status)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parsea_error_1029_de_wattpad() {
        let raw = r#"{"error_code":1029,"error_type":"PermissionDenied","message":"Login failed. Invalid user ID or password"}"#;
        let msg = parsear_error_wattpad(400, raw);
        assert!(msg.contains("1029"));
        assert!(msg.contains("incorrectos"));
    }

    #[test]
    fn parsea_respuesta_de_sesion() {
        let session_raw = r#"{"token":"abc123","user":{"username":"lector_linux","name":"Lector"}}"#;
        let session: SessionApiResponse = serde_json::from_str(session_raw).unwrap();
        assert_eq!(session.token.as_deref(), Some("abc123"));
        assert_eq!(
            session.user.unwrap().username.as_deref(),
            Some("lector_linux")
        );
    }

    #[test]
    fn inyecta_cookie_token_persistente() {
        let mut store = CookieStore::new();
        let wp_url = reqwest::Url::parse("https://www.wattpad.com/").unwrap();
        let cookie_header =
            "token=tok_demo_123; Domain=.wattpad.com; Path=/; Max-Age=2592000; Secure; HttpOnly";
        store.parse(cookie_header, &wp_url).unwrap();

        assert!(store
            .iter_unexpired()
            .any(|c| c.name() == "token" && c.value() == "tok_demo_123" && c.is_persistent()));
    }
}
