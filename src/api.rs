// src/api.rs
use crate::crypto;
use crate::db::{self, DbChapter, DbStory};
use crate::sesion;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::Duration;
use wp_mini::field::{PartStubField, StoryField, UserStubField};
use wp_mini::WattpadClient;

static SHARED_HTTP_CLIENT: OnceLock<reqwest::Client> = OnceLock::new();

#[derive(Debug, Clone, Default)]
pub struct UserProfile {
    pub username: String,
    pub full_name: String,
    pub avatar_url: String,
    pub followers: u64,
    pub following: u64,
    pub stories_published: u64,
    pub description: String,
}

#[derive(Debug, Clone)]
pub struct LibraryStory {
    pub id: String,
    pub title: String,
    pub author: String,
    pub cover_url: String,
}

#[derive(Debug, Clone)]
pub struct SearchStory {
    pub id: String,
    pub title: String,
    pub author: String,
    pub cover_url: String,
    pub reads: u64,
    pub votes: u64,
    pub parts: u32,
    pub is_completed: bool,
    pub is_mature: bool,
    pub description: String,
}

#[derive(Debug, Clone)]
pub struct Chapter {
    pub id: String,
    pub title: String,
    pub index: usize,
}

#[derive(Debug, Clone)]
pub struct StoryDetails {
    pub id: String,
    pub title: String,
    pub author: String,
    pub cover_url: String,
    pub description: String,
    pub chapters: Vec<Chapter>,
}

pub async fn obtener_detalles_historia(
    client: &WattpadClient,
    story_id: &str,
) -> Result<StoryDetails, String> {
    if let Some(story) = historias_destacadas_simuladas()
        .into_iter()
        .find(|story| story.id == story_id)
    {
        return Ok(StoryDetails {
            id: story.id.clone(),
            title: story.title,
            author: story.author,
            cover_url: story.cover_url,
            description: "Historia destacada disponible como vista previa local.".into(),
            chapters: vec![Chapter {
                id: format!("{}-cap-1", story.id),
                title: "Capítulo 1".into(),
                index: 0,
            }],
        });
    }

    let _ = client;
    let http = obtener_http_client();
    let url = format!("https://www.wattpad.com/api/v3/stories/{}", story_id);
    let response = http
        .get(url)
        .query(&[(
            "fields",
            "id,title,description,cover,user(name,fullname),parts(id,title)",
        )])
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| format!("Error de red al obtener detalles de la historia: {e}"))?;
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|e| format!("Error al leer los detalles: {e}"))?;
    if !status.is_success() {
        return Err(sesion::parsear_error_wattpad(status.as_u16(), &body));
    }
    let value: serde_json::Value = serde_json::from_str(&body)
        .map_err(|e| format!("Respuesta inválida al obtener detalles: {e}"))?;
    let value_string = |v: &serde_json::Value| {
        v.as_str()
            .map(str::to_owned)
            .or_else(|| v.as_u64().map(|n| n.to_string()))
    };
    let user = value.get("user").and_then(serde_json::Value::as_object);
    let author = user
        .and_then(|u| u.get("fullname").or_else(|| u.get("name")))
        .and_then(serde_json::Value::as_str)
        .filter(|s| !s.is_empty())
        .unwrap_or("Autor desconocido")
        .to_owned();
    let chapters = value
        .get("parts")
        .and_then(serde_json::Value::as_array)
        .map(|parts| {
            parts
                .iter()
                .enumerate()
                .filter_map(|(index, part)| {
                    let id = value_string(part.get("id")?)?;
                    let title = part
                        .get("title")
                        .and_then(serde_json::Value::as_str)
                        .filter(|title| !title.trim().is_empty())
                        .map(str::to_owned)
                        .unwrap_or_else(|| format!("Capítulo {}", index + 1));
                    Some(Chapter { id, title, index })
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(StoryDetails {
        id: value
            .get("id")
            .and_then(value_string)
            .unwrap_or_else(|| story_id.to_owned()),
        title: value
            .get("title")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("Historia sin título")
            .to_owned(),
        author,
        cover_url: value
            .get("cover")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        description: value
            .get("description")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        chapters,
    })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UserProfileApiResponse {
    username: Option<String>,
    name: Option<String>,
    #[serde(rename = "fullname")]
    full_name: Option<String>,
    avatar: Option<String>,
    description: Option<String>,
    num_followers: Option<u64>,
    num_following: Option<u64>,
    num_stories_published: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct LibraryApiResponse {
    #[serde(default)]
    stories: Vec<LibraryStoryApiItem>,
    #[serde(default)]
    lists: Vec<ReadingListApiItem>,
}

#[derive(Debug, Deserialize)]
struct ReadingListApiItem {
    #[serde(default)]
    stories: Vec<LibraryStoryApiItem>,
}

#[derive(Debug, Deserialize)]
struct LibraryStoryApiItem {
    id: Option<String>,
    title: Option<String>,
    cover: Option<String>,
    user: Option<SearchStoryAuthor>,
}

#[derive(Debug, Deserialize)]
struct SearchResponse {
    #[serde(default)]
    stories: Vec<SearchStoryItem>,
}

#[derive(Debug, Deserialize)]
struct SearchStoryItem {
    id: String,
    title: String,
    #[serde(default)]
    cover: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default, rename = "numReads")]
    num_reads: u64,
    #[serde(default, rename = "numParts")]
    num_parts: u64,
    #[serde(default, rename = "voteCount")]
    vote_count: u64,
    #[serde(default)]
    completed: bool,
    #[serde(default)]
    mature: bool,
    user: Option<SearchStoryAuthor>,
}

#[derive(Debug, Deserialize)]
struct SearchStoryAuthor {
    name: Option<String>,
    fullname: Option<String>,
}

/// Construye un `WattpadClient` de `wp-mini` usando el `reqwest::Client` con cookies compartidas.
pub fn crear_wattpad_client(http_client: reqwest::Client) -> WattpadClient {
    let _ = SHARED_HTTP_CLIENT.set(http_client.clone());
    WattpadClient::builder().reqwest_client(http_client).build()
}

fn obtener_http_client() -> &'static reqwest::Client {
    SHARED_HTTP_CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .user_agent(sesion::USER_AGENT)
            .timeout(Duration::from_secs(20))
            .build()
            .expect("No se pudo construir el cliente HTTP auxiliar")
    })
}

/// Busca historias en Wattpad asumiendo que el cliente ya está autenticado.
///
/// Devuelve los resultados formateados para la interfaz de usuario.
pub async fn buscar_historias(
    query: &str,
    filtros: &str,
    client: &WattpadClient,
) -> Result<Vec<SearchStory>, String> {
    let q = query.trim();
    if q.is_empty() {
        return Ok(Vec::new());
    }

    // TODO: Implementar según wp-mini cuando el crate soporte búsqueda de historias
    // y exponga su sesión/cliente HTTP. En wp-mini 0.2.0-alpha.3 `WattpadClient` solo
    // expone `client.user` y `client.story` (por ID) y mantiene su `http` privado.
    let _ = client;
    let http = obtener_http_client();

    let mut params = vec![
        ("query".to_string(), q.to_string()),
        ("limit".to_string(), "20".to_string()),
        (
            "fields".to_string(),
            "stories(id,title,cover,description,numReads,numParts,voteCount,completed,mature,user(name,fullname))".to_string(),
        ),
    ];
    let opciones: std::collections::HashMap<&str, &str> = filtros
        .split(';')
        .filter_map(|option| option.split_once('='))
        .collect();

    match opciones.get("length").copied().unwrap_or("0") {
        "1" => params.extend([
            ("minParts".into(), "1".into()),
            ("maxParts".into(), "10".into()),
        ]),
        "2" => params.extend([
            ("minParts".into(), "10".into()),
            ("maxParts".into(), "20".into()),
        ]),
        "3" => params.extend([
            ("minParts".into(), "20".into()),
            ("maxParts".into(), "50".into()),
        ]),
        "4" => params.push(("minParts".into(), "50".into())),
        _ => {}
    }

    if opciones.get("updated").copied().unwrap_or("0") != "0" {
        let updated = match opciones.get("updated").copied().unwrap_or("0") {
            "1" => "today",
            "2" => "week",
            "3" => "month",
            "4" => "year",
            _ => "",
        };
        if !updated.is_empty() {
            // TODO: Confirmar el nombre y los valores aceptados por Wattpad para el filtro de actualización.
            params.push(("lastUpdated".into(), updated.into()));
        }
    }

    if opciones.get("completed") == Some(&"true") {
        params.push(("filter".into(), "complete".into()));
    }
    if opciones.get("hide_mature") == Some(&"true") {
        params.push(("mature".into(), "false".into()));
    }
    if opciones.get("hide_free") == Some(&"true") {
        // TODO: Confirmar el parámetro de Wattpad para excluir historias gratuitas.
        params.push(("paid".into(), "true".into()));
    }
    if let Some(tags) = opciones
        .get("tags")
        .map(|tags| tags.trim())
        .filter(|tags| !tags.is_empty())
    {
        // TODO: Confirmar si Wattpad admite un parámetro tags independiente en esta ruta.
        params.push(("tags".into(), tags.to_string()));
    }

    let response = http
        .get("https://api.wattpad.com/v4/search/stories")
        .query(&params)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| format!("Error de red al buscar historias: {}", e))?;

    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|e| format!("Error al leer resultados de búsqueda: {}", e))?;

    if !status.is_success() {
        return Err(sesion::parsear_error_wattpad(status.as_u16(), &body));
    }

    let parsed: SearchResponse = serde_json::from_str(&body)
        .map_err(|e| format!("Error al procesar resultados de Wattpad: {}", e))?;

    let resultados = parsed
        .stories
        .into_iter()
        .map(|s| {
            let author = s
                .user
                .and_then(|u| u.fullname.filter(|n| !n.is_empty()).or(u.name))
                .unwrap_or_else(|| "Autor desconocido".to_string());
            SearchStory {
                id: s.id,
                title: s.title,
                author,
                cover_url: s.cover.unwrap_or_default(),
                reads: s.num_reads,
                votes: s.vote_count,
                parts: s.num_parts.min(u32::MAX as u64) as u32,
                is_completed: s.completed,
                is_mature: s.mature,
                description: s.description.unwrap_or_default(),
            }
        })
        .collect();

    Ok(resultados)
}

/// Obtiene el nombre de usuario de la sesión HTTP actual si existe una sesión activa.
pub async fn obtener_usuario_actual() -> Option<String> {
    let http = SHARED_HTTP_CLIENT.get()?;
    sesion::verificar_sesion(http).await.ok()
}

/// Consulta las historias publicadas de un usuario mediante `wp-mini` y devuelve `(id, titulo, votos)`.
pub async fn obtener_votos_historias_usuario(
    client: &WattpadClient,
    username: &str,
) -> Result<Vec<(String, String, u64)>, String> {
    let fields = [StoryField::Id, StoryField::Title, StoryField::VoteCount];
    let paginator = client
        .user
        .get_user_stories(username, Some(&fields))
        .map_err(|e| format!("Error al preparar consulta de historias: {}", e))?;

    let stories = paginator
        .execute_all()
        .await
        .map_err(|e| format!("Error al obtener historias del usuario: {}", e))?;

    Ok(stories
        .into_iter()
        .filter_map(|story| {
            let id = story.id?;
            let title = story.title.unwrap_or_else(|| "Sin título".to_string());
            let votes = story.vote_count.unwrap_or(0);
            Some((id, title, votes))
        })
        .collect())
}

/// Consulta una historia mediante `wp-mini` y devuelve `(titulo, num_capitulos)`.
pub async fn obtener_capitulos_historia(
    client: &WattpadClient,
    story_id: u64,
) -> Result<(String, u64), String> {
    let fields = [StoryField::Id, StoryField::Title, StoryField::NumParts];
    let story = client
        .story
        .get_story_info(story_id, Some(&fields))
        .await
        .map_err(|e| format!("Error al obtener historia {}: {}", story_id, e))?;

    let title = story
        .title
        .unwrap_or_else(|| format!("Historia {}", story_id));
    let parts = story.num_parts.unwrap_or(0);
    Ok((title, parts))
}

/// Obtiene el perfil del usuario autenticado.
pub async fn obtener_perfil(client: &WattpadClient) -> Result<UserProfile, String> {
    let _ = client;
    let http = obtener_http_client();

    // TODO: Confirmar si `/api/v3/users/me` acepta todos los campos privados en todas las cuentas;
    // actualmente `/api/v3/internal/current_user` devuelve la sesión activa con cookies.
    let response = http
        .get("https://www.wattpad.com/api/v3/internal/current_user")
        .query(&[(
            "fields",
            "username,name,fullname,avatar,description,numFollowers,numFollowing,numStoriesPublished",
        )])
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| format!("Error de red al obtener perfil: {}", e))?;

    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|e| format!("Error al leer perfil de usuario: {}", e))?;

    if !status.is_success() {
        return Err(sesion::parsear_error_wattpad(status.as_u16(), &body));
    }

    let parsed: UserProfileApiResponse = serde_json::from_str(&body)
        .map_err(|e| format!("Error al procesar perfil de Wattpad: {}", e))?;

    let username = parsed.username.or(parsed.name).unwrap_or_default();

    Ok(UserProfile {
        full_name: parsed.full_name.unwrap_or_else(|| username.clone()),
        username,
        avatar_url: parsed.avatar.unwrap_or_default(),
        followers: parsed.num_followers.unwrap_or(0),
        following: parsed.num_following.unwrap_or(0),
        stories_published: parsed.num_stories_published.unwrap_or(0),
        description: parsed.description.unwrap_or_default(),
    })
}

fn parsear_historias_library_response(body: &str) -> Result<Vec<LibraryStory>, String> {
    let parsed: LibraryApiResponse = serde_json::from_str(body)
        .map_err(|e| format!("Error al procesar respuesta de historias de Wattpad: {}", e))?;

    let raw_items = if !parsed.stories.is_empty() {
        parsed.stories
    } else {
        parsed
            .lists
            .into_iter()
            .flat_map(|list| list.stories)
            .collect()
    };

    let mut seen_ids = HashSet::new();
    let mut historias = Vec::new();
    for item in raw_items {
        let Some(id) = item.id.filter(|s| !s.is_empty()) else {
            continue;
        };
        if !seen_ids.insert(id.clone()) {
            continue;
        }
        let author = item
            .user
            .and_then(|u| u.fullname.filter(|n| !n.is_empty()).or(u.name))
            .unwrap_or_else(|| "Autor desconocido".to_string());
        historias.push(LibraryStory {
            id,
            title: item.title.unwrap_or_else(|| "Sin título".to_string()),
            author,
            cover_url: item.cover.unwrap_or_default(),
        });
    }

    Ok(historias)
}

fn historias_destacadas_simuladas() -> Vec<LibraryStory> {
    vec![
        LibraryStory {
            id: "demo-1".to_string(),
            title: "Crónicas del Bosque de Niebla".to_string(),
            author: "Elena Valdés".to_string(),
            cover_url: String::new(),
        },
        LibraryStory {
            id: "demo-2".to_string(),
            title: "El Último Tren a Orión".to_string(),
            author: "Mateo Roldán".to_string(),
            cover_url: String::new(),
        },
        LibraryStory {
            id: "demo-3".to_string(),
            title: "Cartas Bajo la Lluvia".to_string(),
            author: "Sofía Márquez".to_string(),
            cover_url: String::new(),
        },
    ]
}

/// Obtiene historias recomendadas o populares para el feed de inicio.
pub async fn obtener_destacados(client: &WattpadClient) -> Result<Vec<LibraryStory>, String> {
    let _ = client;
    let http = obtener_http_client();

    // TODO: Confirmar el endpoint exacto de recomendaciones personalizadas de Wattpad
    // (actualmente se consulta `/v4/stories?filter=hot` y se usan 3 historias simuladas como respaldo).
    let response = http
        .get("https://api.wattpad.com/v4/stories")
        .query(&[
            ("filter", "hot"),
            ("limit", "15"),
            ("fields", "stories(id,title,cover,user(name,fullname))"),
        ])
        .header("Accept", "application/json")
        .send()
        .await;

    if let Ok(resp) = response {
        if resp.status().is_success() {
            if let Ok(body) = resp.text().await {
                if let Ok(historias) = parsear_historias_library_response(&body) {
                    if !historias.is_empty() {
                        return Ok(historias);
                    }
                }
            }
        }
    }

    Ok(historias_destacadas_simuladas())
}

/// Obtiene las historias de la biblioteca / listas de lectura del usuario autenticado.
pub async fn obtener_biblioteca(client: &WattpadClient) -> Result<Vec<LibraryStory>, String> {
    let _ = client;
    let http = obtener_http_client();

    // TODO: Confirmar el endpoint exacto de la biblioteca privada (`/api/v3/library` vs
    // `/api/v3/users/{username}/lists`), ya que `wp-mini` aún no incluye lectura de biblioteca.
    let mut response = http
        .get("https://www.wattpad.com/api/v3/library")
        .query(&[
            ("limit", "30"),
            ("fields", "stories(id,title,cover,user(name,fullname))"),
        ])
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| format!("Error de red al obtener biblioteca: {}", e))?;

    if !response.status().is_success() {
        if let Some(username) = obtener_usuario_actual().await {
            let url = format!("https://www.wattpad.com/api/v3/users/{}/lists", username);
            response = http
                .get(&url)
                .query(&[(
                    "fields",
                    "lists(stories(id,title,cover,user(name,fullname)))",
                )])
                .header("Accept", "application/json")
                .send()
                .await
                .map_err(|e| format!("Error de red al obtener listas de lectura: {}", e))?;
        }
    }

    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|e| format!("Error al leer biblioteca de Wattpad: {}", e))?;

    if !status.is_success() {
        return Err(sesion::parsear_error_wattpad(status.as_u16(), &body));
    }

    parsear_historias_library_response(&body)
}

fn ruta_cache_imagen(nombre_archivo: &str) -> PathBuf {
    let base = if let Ok(xdg_cache) = std::env::var("XDG_CACHE_HOME") {
        if !xdg_cache.trim().is_empty() {
            PathBuf::from(xdg_cache).join("mi-cliente-wattpad")
        } else {
            std::env::temp_dir().join("mi-cliente-wattpad")
        }
    } else if let Ok(home) = std::env::var("HOME") {
        if !home.trim().is_empty() {
            PathBuf::from(home)
                .join(".cache")
                .join("mi-cliente-wattpad")
        } else {
            std::env::temp_dir().join("mi-cliente-wattpad")
        }
    } else {
        std::env::temp_dir().join("mi-cliente-wattpad")
    };

    base.join("images").join(nombre_archivo)
}

/// Descarga una imagen remota a caché local para cargarla desde Slint con `Image::load_from_path`.
pub async fn descargar_imagen_cache(url: &str, clave: &str) -> Option<PathBuf> {
    let url = url.trim();
    if url.is_empty() {
        return None;
    }

    let safe_key: String = clave
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let ruta = ruta_cache_imagen(&format!("{}.img", safe_key));
    if ruta.exists() {
        return Some(ruta);
    }

    if let Some(parent) = ruta.parent() {
        std::fs::create_dir_all(parent).ok()?;
    }

    let http = obtener_http_client();
    let response = http.get(url).send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }
    let bytes = response.bytes().await.ok()?;
    std::fs::write(&ruta, &bytes).ok()?;
    Some(ruta)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpubData {
    pub story: DbStory,
    pub chapters: Vec<DbChapter>,
}

pub fn ruta_epub(story_id: &str) -> PathBuf {
    let base = if let Ok(xdg_data) = std::env::var("XDG_DATA_HOME") {
        if !xdg_data.trim().is_empty() {
            PathBuf::from(xdg_data).join("mi-cliente-wattpad")
        } else {
            PathBuf::from("mi-cliente-wattpad")
        }
    } else if let Ok(home) = std::env::var("HOME") {
        if !home.trim().is_empty() {
            PathBuf::from(home)
                .join(".local")
                .join("share")
                .join("mi-cliente-wattpad")
        } else {
            PathBuf::from("mi-cliente-wattpad")
        }
    } else {
        PathBuf::from("mi-cliente-wattpad")
    };

    let safe_id: String = story_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();

    base.join("epubs").join(format!("{}.epub", safe_id))
}

pub fn epub_descargado(story_id: &str) -> bool {
    ruta_epub(story_id).is_file()
}

/// Serializa y encripta la historia como archivo EPUB en disco anteponiendo `salt` y `nonce`.
pub fn guardar_epub_encriptado(
    story: &DbStory,
    chapters: &[DbChapter],
    username: &str,
    password: &str,
    salt: &[u8],
) -> Result<PathBuf, String> {
    if salt.len() != crypto::SALT_LEN {
        return Err(format!(
            "Salt inválido: se esperaban {} bytes y se recibieron {}",
            crypto::SALT_LEN,
            salt.len()
        ));
    }

    let payload = EpubData {
        story: story.clone(),
        chapters: chapters.to_vec(),
    };
    let datos_planos = serde_json::to_vec(&payload)
        .map_err(|e| format!("Error al serializar contenido del EPUB: {}", e))?;

    let clave = crypto::derivar_clave(username, password, salt);
    let (datos_cifrados, nonce) = crypto::encriptar_epub(&datos_planos, &clave)?;

    let mut contenido_archivo =
        Vec::with_capacity(crypto::SALT_LEN + crypto::NONCE_LEN + datos_cifrados.len());
    contenido_archivo.extend_from_slice(salt);
    contenido_archivo.extend_from_slice(&nonce);
    contenido_archivo.extend_from_slice(&datos_cifrados);

    let ruta = ruta_epub(&story.id);
    if let Some(parent) = ruta.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Error al crear directorio de EPUBs: {}", e))?;
    }
    std::fs::write(&ruta, &contenido_archivo)
        .map_err(|e| format!("Error al guardar archivo EPUB cifrado: {}", e))?;

    Ok(ruta)
}

/// Lee un archivo EPUB cifrado, extrae `salt` y `nonce`, deriva la clave y devuelve los capítulos desencriptados.
pub fn leer_epub_desencriptado(
    story_id: &str,
    username: &str,
    password: &str,
) -> Result<EpubData, String> {
    let ruta = ruta_epub(story_id);
    let bytes = std::fs::read(&ruta).map_err(|e| {
        format!(
            "No se pudo leer el archivo EPUB '{}': {}",
            ruta.display(),
            e
        )
    })?;

    let header_len = crypto::SALT_LEN + crypto::NONCE_LEN;
    if bytes.len() <= header_len {
        return Err("El archivo EPUB está corrupto o incompleto.".to_string());
    }

    let salt = &bytes[..crypto::SALT_LEN];
    let nonce = &bytes[crypto::SALT_LEN..header_len];
    let datos_cifrados = &bytes[header_len..];

    let clave = crypto::derivar_clave(username, password, salt);
    let datos_planos = crypto::desencriptar_epub(datos_cifrados, nonce, &clave)?;

    serde_json::from_slice(&datos_planos)
        .map_err(|e| format!("Error al decodificar contenido del EPUB: {}", e))
}

/// Descarga metadatos y capítulos de una historia desde Wattpad.
async fn obtener_historia_y_capitulos_api(
    client: &WattpadClient,
    story_id: &str,
    pausa_entre_capitulos: bool,
) -> Result<(DbStory, Vec<DbChapter>), String> {
    let story_id_num: u64 = story_id
        .trim()
        .parse()
        .map_err(|_| format!("ID de historia inválido: '{}'", story_id))?;

    let fields = [
        StoryField::Id,
        StoryField::Title,
        StoryField::Cover,
        StoryField::User(vec![UserStubField::Username, UserStubField::FullName]),
        StoryField::Parts(vec![PartStubField::Id, PartStubField::Title]),
    ];

    let story = client
        .story
        .get_story_info(story_id_num, Some(&fields))
        .await
        .map_err(|e| {
            format!(
                "Error al obtener detalles de la historia {}: {}",
                story_id, e
            )
        })?;

    let title = story
        .title
        .unwrap_or_else(|| format!("Historia {}", story_id));
    let author = story
        .user
        .and_then(|u| u.fullname.filter(|n| !n.is_empty()).or(u.username))
        .unwrap_or_else(|| "Autor desconocido".to_string());
    let cover_url = story.cover.unwrap_or_default();
    let parts = story.parts.unwrap_or_default();

    let mut db_chapters = Vec::with_capacity(parts.len());
    for (idx, part) in parts.into_iter().enumerate() {
        let Some(part_id) = part.id else {
            continue;
        };

        // TODO: Verificar si futuras versiones de wp-mini unifican el acceso en `client.story.get_content()`;
        // en wp-mini 0.2.0-alpha.3 los métodos disponibles son `get_part_content_json` y `get_part_content_raw`.
        let raw_text = match client.story.get_part_content_json(part_id).await {
            Ok(content) => match content.text {
                Some(text) if !text.trim().is_empty() => text,
                _ => client
                    .story
                    .get_part_content_raw(part_id)
                    .await
                    .map_err(|e| format!("Error descargando capítulo {}: {}", part_id, e))?,
            },
            Err(_) => client
                .story
                .get_part_content_raw(part_id)
                .await
                .map_err(|e| format!("Error descargando capítulo {}: {}", part_id, e))?,
        };

        let chapter_title = part
            .title
            .filter(|t| !t.trim().is_empty())
            .unwrap_or_else(|| format!("Capítulo {}", idx + 1));

        db_chapters.push(DbChapter {
            id: part_id.to_string(),
            story_id: story_id.to_string(),
            chapter_number: (idx + 1) as i32,
            title: chapter_title,
            content: raw_text,
        });

        if pausa_entre_capitulos {
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        }
    }

    let db_story = DbStory {
        id: story_id.to_string(),
        title,
        author,
        cover_url,
    };

    Ok((db_story, db_chapters))
}

/// Obtiene una historia y sus capítulos desde la API (o desde datos simulados si es historia demo).
pub async fn cargar_historia_para_lectura_api(
    client: &WattpadClient,
    story_id: &str,
) -> Result<(String, Vec<DbChapter>), String> {
    if let Some(demo) = historias_destacadas_simuladas()
        .into_iter()
        .find(|s| s.id == story_id)
    {
        let chapters = vec![DbChapter {
            id: format!("{}-cap-1", demo.id),
            story_id: demo.id,
            chapter_number: 1,
            title: "Capítulo 1".to_string(),
            content: format!(
                "<p><strong>{}</strong> — por <em>{}</em></p><p>Esta es una vista previa generada localmente porque el feed destacado usó datos de prueba sin conexión.</p>",
                demo.title, demo.author
            ),
        }];
        return Ok((demo.title, chapters));
    }

    let (story, chapters) = obtener_historia_y_capitulos_api(client, story_id, false).await?;
    Ok((story.title, chapters))
}

/// Descarga la metadata y capítulos de una historia, los guarda encriptados en un archivo EPUB
/// y registra los metadatos en SQLite.
pub async fn descargar_historia_completa(
    client: &WattpadClient,
    story_id: &str,
    username: &str,
    password: &str,
    salt: &[u8],
) -> Result<usize, String> {
    let (db_story, db_chapters) = obtener_historia_y_capitulos_api(client, story_id, true).await?;
    let count = db_chapters.len();
    let username = username.to_string();
    let password = password.to_string();
    let salt = salt.to_vec();

    tokio::task::spawn_blocking(move || {
        guardar_epub_encriptado(&db_story, &db_chapters, &username, &password, &salt)?;
        let mut conn = db::inicializar_db()?;
        db::guardar_historia_metadata(&mut conn, &db_story)
    })
    .await
    .map_err(|e| format!("Error interno guardando EPUB cifrado: {}", e))??;

    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parsea_respuesta_de_busqueda() {
        let search_raw = r#"{"stories":[{"id":"42","title":"Historia Linux","numParts":10,"voteCount":500,"user":{"name":"autor"}}]}"#;
        let search: SearchResponse = serde_json::from_str(search_raw).unwrap();
        assert_eq!(search.stories.len(), 1);
        assert_eq!(search.stories[0].title, "Historia Linux");
    }

    #[test]
    fn guarda_y_lee_epub_encriptado_con_salt_y_nonce() {
        let dir = std::env::temp_dir().join(format!("wattpad_epub_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::env::set_var("XDG_DATA_HOME", &dir);

        let story = DbStory {
            id: "998877".to_string(),
            title: "Historia Cifrada".to_string(),
            author: "Autora".to_string(),
            cover_url: String::new(),
        };
        let chapters = vec![DbChapter {
            id: "1".to_string(),
            story_id: "998877".to_string(),
            chapter_number: 1,
            title: "Cap 1".to_string(),
            content: "Hola **mundo**.".to_string(),
        }];
        let salt = crypto::generar_salt();

        let ruta = guardar_epub_encriptado(&story, &chapters, "user1", "pass1", &salt).unwrap();
        assert!(ruta.is_file());

        let leido = leer_epub_desencriptado("998877", "user1", "pass1").unwrap();
        assert_eq!(leido.story, story);
        assert_eq!(leido.chapters, chapters);
        assert!(leer_epub_desencriptado("998877", "user1", "otra_clave").is_err());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
