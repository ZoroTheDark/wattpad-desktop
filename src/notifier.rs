// src/notifier.rs
use crate::api;
use notify_rust::Notification;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use wp_mini::WattpadClient;

const POLL_INTERVAL_SECS: u64 = 300; // 5 minutos

#[derive(Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
struct NotificationState {
    #[serde(default)]
    votos_por_historia: HashMap<String, u64>,
    #[serde(default)]
    capitulos_por_historia: HashMap<String, u64>,
    #[serde(default)]
    historias_seguidas: Vec<u64>,
}

fn ruta_estado_notificaciones() -> PathBuf {
    if let Ok(xdg_data) = std::env::var("XDG_DATA_HOME") {
        if !xdg_data.trim().is_empty() {
            return PathBuf::from(xdg_data)
                .join("mi-cliente-wattpad")
                .join("estado_notificaciones.json");
        }
    }

    if let Ok(home) = std::env::var("HOME") {
        if !home.trim().is_empty() {
            return PathBuf::from(home)
                .join(".local")
                .join("share")
                .join("mi-cliente-wattpad")
                .join("estado_notificaciones.json");
        }
    }

    PathBuf::from("estado_notificaciones.json")
}

fn cargar_estado(ruta: &Path) -> NotificationState {
    std::fs::File::open(ruta)
        .ok()
        .and_then(|file| serde_json::from_reader(std::io::BufReader::new(file)).ok())
        .unwrap_or_default()
}

fn guardar_estado(ruta: &Path, estado: &NotificationState) {
    if let Some(parent) = ruta.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            eprintln!("Error al crear directorio de notificaciones: {}", e);
            return;
        }
    }

    match std::fs::File::create(ruta) {
        Ok(file) => {
            if let Err(e) = serde_json::to_writer_pretty(std::io::BufWriter::new(file), estado) {
                eprintln!("Error al guardar estado_notificaciones.json: {}", e);
            }
        }
        Err(e) => eprintln!("Error al crear estado_notificaciones.json: {}", e),
    }
}

fn evaluar_incremento(anterior: Option<u64>, actual: u64) -> bool {
    matches!(anterior, Some(prev) if actual > prev)
}

fn enviar_notificacion(cuerpo: &str) {
    if let Err(e) = Notification::new()
        .summary("Wattpad")
        .body(cuerpo)
        .show()
    {
        eprintln!("No se pudo mostrar la notificación: {}", e);
    }
}

async fn verificar_cambios(client: &WattpadClient, ruta_estado: &Path) {
    let mut estado = cargar_estado(ruta_estado);
    let mut modificado = false;

    // 1. Votos en las historias del usuario autenticado
    // TODO: Consultar también el endpoint interno de notificaciones/estadísticas de Wattpad
    // si se requiere detectar votos en historias privadas o borradores no listados en /stories.
    if let Some(username) = api::obtener_usuario_actual().await {
        match api::obtener_votos_historias_usuario(client, &username).await {
            Ok(historias) => {
                for (story_id, title, votos_actuales) in historias {
                    let anterior = estado.votos_por_historia.get(&story_id).copied();
                    if evaluar_incremento(anterior, votos_actuales) {
                        enviar_notificacion(&format!(
                            "¡Tu historia {} recibió un nuevo voto!",
                            title
                        ));
                    }
                    if anterior != Some(votos_actuales) {
                        estado.votos_por_historia.insert(story_id, votos_actuales);
                        modificado = true;
                    }
                }
            }
            Err(e) => eprintln!("Error verificando votos del usuario: {}", e),
        }
    }

    // 2. Nuevos capítulos en historias seguidas o de la biblioteca
    // TODO: Llamar al endpoint de biblioteca / listas de lectura de Wattpad (ej. /api/v3/users/{user}/lists)
    // para poblar `estado.historias_seguidas`, ya que `wp-mini` 0.2.0-alpha.3 aún no lo incluye.
    let seguidas = estado.historias_seguidas.clone();
    for story_id in seguidas {
        match api::obtener_capitulos_historia(client, story_id).await {
            Ok((title, capitulos_actuales)) => {
                let key = story_id.to_string();
                let anterior = estado.capitulos_por_historia.get(&key).copied();
                if evaluar_incremento(anterior, capitulos_actuales) {
                    enviar_notificacion(&format!("¡Nuevo capítulo en {}!", title));
                }
                if anterior != Some(capitulos_actuales) {
                    estado.capitulos_por_historia.insert(key, capitulos_actuales);
                    modificado = true;
                }
            }
            Err(e) => eprintln!("Error verificando capítulos de {}: {}", story_id, e),
        }
    }

    if modificado {
        guardar_estado(ruta_estado, &estado);
    }
}

/// Inicia el sondeo periódico en segundo plano (cada 5 minutos) usando `WattpadClient`.
pub async fn iniciar_notificaciones(client: Arc<WattpadClient>) {
    let ruta_estado = ruta_estado_notificaciones();

    loop {
        verificar_cambios(&client, &ruta_estado).await;
        tokio::time::sleep(Duration::from_secs(POLL_INTERVAL_SECS)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detecta_incremento_de_contadores() {
        assert!(!evaluar_incremento(None, 5));
        assert!(!evaluar_incremento(Some(5), 5));
        assert!(!evaluar_incremento(Some(5), 4));
        assert!(evaluar_incremento(Some(5), 6));
    }

    #[test]
    fn serializa_y_deserializa_estado_notificaciones() {
        let mut estado = NotificationState::default();
        estado.votos_por_historia.insert("101".to_string(), 12);
        estado.capitulos_por_historia.insert("202".to_string(), 8);
        estado.historias_seguidas.push(202);

        let json = serde_json::to_string(&estado).unwrap();
        let recuperado: NotificationState = serde_json::from_str(&json).unwrap();
        assert_eq!(estado, recuperado);
    }
}
