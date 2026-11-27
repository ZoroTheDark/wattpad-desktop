// src/db.rs
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DbStory {
    pub id: String,
    pub title: String,
    pub author: String,
    pub cover_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DbChapter {
    pub id: String,
    pub story_id: String,
    pub chapter_number: i32,
    pub title: String,
    pub content: String,
}

pub fn ruta_db() -> PathBuf {
    if let Ok(xdg_data) = std::env::var("XDG_DATA_HOME") {
        if !xdg_data.trim().is_empty() {
            return PathBuf::from(xdg_data)
                .join("mi-cliente-wattpad")
                .join("wattpad.db");
        }
    }

    if let Ok(home) = std::env::var("HOME") {
        if !home.trim().is_empty() {
            return PathBuf::from(home)
                .join(".local")
                .join("share")
                .join("mi-cliente-wattpad")
                .join("wattpad.db");
        }
    }

    PathBuf::from("wattpad.db")
}

/// Inicializa la base de datos SQLite principal (`wattpad.db`) y crea las tablas necesarias.
pub fn inicializar_db() -> Result<Connection, String> {
    inicializar_db_en_ruta(&ruta_db())
}

pub fn inicializar_db_en_ruta(ruta: &Path) -> Result<Connection, String> {
    if let Some(parent) = ruta.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Error al crear directorio de base de datos: {}", e))?;
        }
    }

    let conn = Connection::open(ruta)
        .map_err(|e| format!("No se pudo abrir la base de datos SQLite: {}", e))?;

    crear_esquema(&conn)?;
    Ok(conn)
}

fn crear_esquema(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        "
        PRAGMA foreign_keys = ON;
        CREATE TABLE IF NOT EXISTS stories (
            id TEXT PRIMARY KEY,
            title TEXT NOT NULL,
            author TEXT NOT NULL,
            cover_url TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS chapters (
            id TEXT PRIMARY KEY,
            story_id TEXT NOT NULL,
            chapter_number INTEGER NOT NULL,
            title TEXT NOT NULL,
            content TEXT NOT NULL,
            FOREIGN KEY (story_id) REFERENCES stories(id) ON DELETE CASCADE
        );
        CREATE INDEX IF NOT EXISTS idx_chapters_story_order
            ON chapters (story_id, chapter_number);
        CREATE TABLE IF NOT EXISTS progress (
            story_id TEXT PRIMARY KEY,
            last_chapter_index INTEGER NOT NULL
        );
        ",
    )
    .map_err(|e| format!("Error al crear tablas en SQLite: {}", e))
}

/// Guarda o actualiza una historia junto con todos sus capítulos en SQLite.
#[allow(dead_code)]
pub fn guardar_historia_con_capitulos(
    conn: &mut Connection,
    story: &DbStory,
    chapters: &[DbChapter],
) -> Result<(), String> {
    let tx = conn
        .transaction()
        .map_err(|e| format!("Error al iniciar transacción SQLite: {}", e))?;

    tx.execute(
        "INSERT OR REPLACE INTO stories (id, title, author, cover_url) VALUES (?1, ?2, ?3, ?4)",
        params![story.id, story.title, story.author, story.cover_url],
    )
    .map_err(|e| format!("Error al guardar historia en SQLite: {}", e))?;

    tx.execute(
        "DELETE FROM chapters WHERE story_id = ?1",
        params![story.id],
    )
    .map_err(|e| format!("Error al limpiar capítulos previos: {}", e))?;

    {
        let mut stmt = tx
            .prepare(
                "INSERT INTO chapters (id, story_id, chapter_number, title, content)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
            )
            .map_err(|e| format!("Error al preparar inserción de capítulos: {}", e))?;

        for ch in chapters {
            stmt.execute(params![
                ch.id,
                ch.story_id,
                ch.chapter_number,
                ch.title,
                ch.content
            ])
            .map_err(|e| format!("Error al guardar capítulo '{}': {}", ch.title, e))?;
        }
    }

    tx.commit()
        .map_err(|e| format!("Error al confirmar transacción SQLite: {}", e))
}

/// Guarda o actualiza solo los metadatos de la historia en SQLite (sin capítulos en texto plano).
pub fn guardar_historia_metadata(conn: &mut Connection, story: &DbStory) -> Result<(), String> {
    let tx = conn
        .transaction()
        .map_err(|e| format!("Error al iniciar transacción SQLite: {}", e))?;

    tx.execute(
        "INSERT OR REPLACE INTO stories (id, title, author, cover_url) VALUES (?1, ?2, ?3, ?4)",
        params![story.id, story.title, story.author, story.cover_url],
    )
    .map_err(|e| format!("Error al guardar metadatos de historia en SQLite: {}", e))?;

    tx.execute(
        "DELETE FROM chapters WHERE story_id = ?1",
        params![story.id],
    )
    .map_err(|e| format!("Error al limpiar capítulos previos: {}", e))?;

    tx.commit()
        .map_err(|e| format!("Error al confirmar transacción SQLite: {}", e))
}

/// Carga todos los capítulos descargados de una historia ordenados por número de capítulo.
pub fn cargar_capitulos_historia(
    conn: &Connection,
    story_id: &str,
) -> Result<Vec<DbChapter>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, story_id, chapter_number, title, content
             FROM chapters
             WHERE story_id = ?1
             ORDER BY chapter_number ASC",
        )
        .map_err(|e| format!("Error al consultar capítulos: {}", e))?;

    let rows = stmt
        .query_map(params![story_id], |row| {
            Ok(DbChapter {
                id: row.get(0)?,
                story_id: row.get(1)?,
                chapter_number: row.get(2)?,
                title: row.get(3)?,
                content: row.get(4)?,
            })
        })
        .map_err(|e| format!("Error leyendo capítulos desde SQLite: {}", e))?;

    let mut capitulos = Vec::new();
    for row in rows {
        capitulos.push(row.map_err(|e| format!("Error al decodificar capítulo: {}", e))?);
    }

    Ok(capitulos)
}

/// Indica si una historia ya tiene al menos un capítulo guardado localmente.
pub fn historia_descargada(conn: &Connection, story_id: &str) -> Result<bool, String> {
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(1) FROM chapters WHERE story_id = ?1",
            params![story_id],
            |row| row.get(0),
        )
        .map_err(|e| format!("Error comprobando descarga en SQLite: {}", e))?;
    Ok(count > 0)
}

/// Devuelve todas las historias almacenadas localmente para lectura offline.
pub fn cargar_historias_descargadas(conn: &Connection) -> Result<Vec<DbStory>, String> {
    let mut stmt = conn
        .prepare("SELECT id, title, author, cover_url FROM stories ORDER BY title ASC")
        .map_err(|e| format!("Error al listar historias locales: {}", e))?;

    let rows = stmt
        .query_map([], |row| {
            Ok(DbStory {
                id: row.get(0)?,
                title: row.get(1)?,
                author: row.get(2)?,
                cover_url: row.get(3)?,
            })
        })
        .map_err(|e| format!("Error leyendo historias locales: {}", e))?;

    let mut stories = Vec::new();
    for row in rows {
        stories.push(row.map_err(|e| format!("Error al decodificar historia: {}", e))?);
    }
    Ok(stories)
}

/// Guarda o actualiza el índice del último capítulo leído para una historia en la conexión dada.
pub fn guardar_progreso_en_conn(
    conn: &Connection,
    story_id: &str,
    index: usize,
) -> Result<(), String> {
    if story_id.trim().is_empty() {
        return Err("El ID de la historia está vacío".to_string());
    }
    let idx_i64 = i64::try_from(index)
        .map_err(|_| "El índice de capítulo excede el rango permitido".to_string())?;
    conn.execute(
        "INSERT OR REPLACE INTO progress (story_id, last_chapter_index) VALUES (?1, ?2)",
        params![story_id, idx_i64],
    )
    .map_err(|e| format!("Error al guardar progreso en SQLite: {}", e))?;
    Ok(())
}

/// Obtiene el último índice de capítulo leído para una historia desde la conexión dada.
pub fn obtener_progreso_en_conn(conn: &Connection, story_id: &str) -> Option<usize> {
    if story_id.trim().is_empty() {
        return None;
    }
    conn.query_row(
        "SELECT last_chapter_index FROM progress WHERE story_id = ?1",
        params![story_id],
        |row| row.get::<_, i64>(0),
    )
    .ok()
    .and_then(|v| usize::try_from(v).ok())
}

/// Guarda o actualiza el último capítulo leído en `wattpad.db`.
pub fn guardar_progreso(story_id: &str, index: usize) -> Result<(), String> {
    let conn = inicializar_db()?;
    guardar_progreso_en_conn(&conn, story_id, index)
}

/// Lee el último capítulo leído desde `wattpad.db`, si existe.
pub fn obtener_progreso(story_id: &str) -> Option<usize> {
    let conn = inicializar_db().ok()?;
    obtener_progreso_en_conn(&conn, story_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guarda_y_carga_historia_y_capitulos_en_sqlite() {
        let mut conn = Connection::open_in_memory().unwrap();
        crear_esquema(&conn).unwrap();

        let story = DbStory {
            id: "123".to_string(),
            title: "Novela de Prueba".to_string(),
            author: "Autora".to_string(),
            cover_url: "https://example.com/cover.jpg".to_string(),
        };
        let chapters = vec![
            DbChapter {
                id: "1001".to_string(),
                story_id: "123".to_string(),
                chapter_number: 1,
                title: "Capítulo 1".to_string(),
                content: "Había una vez...".to_string(),
            },
            DbChapter {
                id: "1002".to_string(),
                story_id: "123".to_string(),
                chapter_number: 2,
                title: "Capítulo 2".to_string(),
                content: "Continuó la aventura.".to_string(),
            },
        ];

        guardar_historia_con_capitulos(&mut conn, &story, &chapters).unwrap();

        assert!(historia_descargada(&conn, "123").unwrap());
        assert_eq!(cargar_historias_descargadas(&conn).unwrap(), vec![story]);

        let loaded = cargar_capitulos_historia(&conn, "123").unwrap();
        assert_eq!(loaded, chapters);
    }

    #[test]
    fn guarda_y_lee_progreso_de_lectura() {
        let conn = Connection::open_in_memory().unwrap();
        crear_esquema(&conn).unwrap();

        assert_eq!(obtener_progreso_en_conn(&conn, "123"), None);

        guardar_progreso_en_conn(&conn, "123", 4).unwrap();
        assert_eq!(obtener_progreso_en_conn(&conn, "123"), Some(4));

        guardar_progreso_en_conn(&conn, "123", 7).unwrap();
        assert_eq!(obtener_progreso_en_conn(&conn, "123"), Some(7));
    }
}
