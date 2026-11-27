// src/main.rs
mod api;
mod crypto;
mod db;
mod lector_web;
mod notifier;
mod sesion;
mod state;

use async_compat::Compat;
use slint::{ComponentHandle, Model};
use state::AppState;
use std::sync::{Arc, Mutex};
use wp_mini::WattpadClient;

slint::include_modules!();

fn actualizar_capitulo_en_ui(app: &AppWindow, state: &AppState) {
    let total = state.opened_chapters.len();
    app.set_reader_story_title(state.opened_story_title.clone().into());
    app.set_reader_total_chapters(total as i32);

    if let Some(chapter) = state.opened_chapters.get(state.current_chapter_index) {
        app.set_reader_chapter_index(state.current_chapter_index as i32);
        app.set_reader_chapter_title(chapter.title.clone().into());
        lector_web::abrir_lector(
            state.opened_story_id.clone(),
            state.opened_story_title.clone(),
            state.opened_chapters.clone(),
            state.current_chapter_index,
        );
        app.set_is_reading(false);
        app.set_is_loading(false);
    }
}

async fn construir_items_biblioteca(historias: &[api::LibraryStory]) -> Vec<LibraryStoryItem> {
    let downloaded_ids: std::collections::HashSet<String> = tokio::task::spawn_blocking(|| {
        let Ok(conn) = db::inicializar_db() else {
            return std::collections::HashSet::new();
        };
        let Ok(locales) = db::cargar_historias_descargadas(&conn) else {
            return std::collections::HashSet::new();
        };
        locales
            .into_iter()
            .filter(|s| db::historia_descargada(&conn, &s.id).unwrap_or(false))
            .map(|s| s.id)
            .collect()
    })
    .await
    .unwrap_or_default();

    let mut ui_items = Vec::with_capacity(historias.len());
    for story in historias {
        let cover = api::descargar_imagen_cache(&story.cover_url, &format!("cover_{}", story.id))
            .await
            .and_then(|path| slint::Image::load_from_path(&path).ok())
            .unwrap_or_default();

        ui_items.push(LibraryStoryItem {
            id: story.id.clone().into(),
            title: story.title.clone().into(),
            author: story.author.clone().into(),
            cover,
            is_downloaded: api::epub_descargado(&story.id) || downloaded_ids.contains(&story.id),
        });
    }
    ui_items
}

fn cargar_datos_usuario(
    app_weak: slint::Weak<AppWindow>,
    state_arc: Arc<Mutex<AppState>>,
    wp_client: Arc<WattpadClient>,
) {
    slint::spawn_local(Compat::new(async move {
        // 1. Cargar perfil de usuario
        match api::obtener_perfil(&wp_client).await {
            Ok(perfil) => {
                let avatar_path = api::descargar_imagen_cache(
                    &perfil.avatar_url,
                    &format!("avatar_{}", perfil.username),
                )
                .await;

                if let Ok(mut state) = state_arc.lock() {
                    state.username = perfil.username.clone();
                    state.user_profile = Some(perfil.clone());
                }

                if let Some(app) = app_weak.upgrade() {
                    app.set_current_username(perfil.username.into());
                    app.set_profile_full_name(perfil.full_name.into());
                    app.set_profile_followers(perfil.followers.min(i32::MAX as u64) as i32);
                    app.set_profile_following(perfil.following.min(i32::MAX as u64) as i32);
                    app.set_profile_stories_count(
                        perfil.stories_published.min(i32::MAX as u64) as i32,
                    );
                    app.set_profile_description(perfil.description.into());
                    if let Some(path) = avatar_path {
                        if let Ok(img) = slint::Image::load_from_path(&path) {
                            app.set_profile_avatar(img);
                        }
                    }
                }
            }
            Err(err) => eprintln!("No se pudo cargar el perfil de usuario: {}", err),
        }

        // 2. Cargar destacados para Home y biblioteca del usuario (con fallback offline a SQLite)
        let destacados = api::obtener_destacados(&wp_client).await.unwrap_or_default();
        let home_ui_items = construir_items_biblioteca(&destacados).await;
        if let Ok(mut state) = state_arc.lock() {
            state.home_stories = destacados;
        }
        if let Some(app) = app_weak.upgrade() {
            app.set_home_stories(slint::ModelRc::new(slint::VecModel::from(home_ui_items)));
        }

        let historias = match api::obtener_biblioteca(&wp_client).await {
            Ok(historias) => historias,
            Err(err) => {
                eprintln!("No se pudo cargar la biblioteca remota: {}", err);
                tokio::task::spawn_blocking(|| {
                    db::inicializar_db()
                        .and_then(|conn| db::cargar_historias_descargadas(&conn))
                        .unwrap_or_default()
                        .into_iter()
                        .map(|s| api::LibraryStory {
                            id: s.id,
                            title: s.title,
                            author: s.author,
                            cover_url: s.cover_url,
                        })
                        .collect()
                })
                .await
                .unwrap_or_default()
            }
        };

        let ui_items = construir_items_biblioteca(&historias).await;
        if let Ok(mut state) = state_arc.lock() {
            state.library_stories = historias;
        }
        if let Some(app) = app_weak.upgrade() {
            app.set_library_stories(slint::ModelRc::new(slint::VecModel::from(ui_items)));
        }
    }))
    .unwrap();
}

fn cargar_destacados_home(
    app_weak: slint::Weak<AppWindow>,
    state_arc: Arc<Mutex<AppState>>,
    wp_client: Arc<WattpadClient>,
) {
    slint::spawn_local(Compat::new(async move {
        if let Ok(destacados) = api::obtener_destacados(&wp_client).await {
            let home_ui_items = construir_items_biblioteca(&destacados).await;
            if let Ok(mut state) = state_arc.lock() {
                state.home_stories = destacados;
            }
            if let Some(app) = app_weak.upgrade() {
                app.set_home_stories(slint::ModelRc::new(slint::VecModel::from(home_ui_items)));
            }
        }
    }))
    .unwrap();
}

fn abrir_historia_para_lectura(
    app_weak: slint::Weak<AppWindow>,
    state_arc: Arc<Mutex<AppState>>,
    wp_opt: Option<Arc<WattpadClient>>,
    story_id: String,
) {
    if let Some(app) = app_weak.upgrade() {
        app.set_is_loading(true);
        app.set_error_message("".into());
    }

    slint::spawn_local(Compat::new(async move {
        let (story_title, username, password) = state_arc
            .lock()
            .ok()
            .map(|s| {
                let title = s
                    .library_stories
                    .iter()
                    .chain(s.home_stories.iter())
                    .find(|st| st.id == story_id)
                    .map(|st| st.title.clone())
                    .unwrap_or_else(|| "Lectura".to_string());
                (title, s.username.clone(), s.password.clone())
            })
            .unwrap_or_else(|| ("Lectura".to_string(), String::new(), String::new()));

        let id_local = story_id.clone();
        let local_result = tokio::task::spawn_blocking(move || {
            if api::epub_descargado(&id_local) {
                let epub = api::leer_epub_desencriptado(&id_local, &username, &password)?;
                Ok(Some((epub.story.title, epub.chapters)))
            } else {
                let conn = db::inicializar_db()?;
                let chapters = db::cargar_capitulos_historia(&conn, &id_local)?;
                if chapters.is_empty() {
                    Ok(None)
                } else {
                    Ok(Some((story_title, chapters)))
                }
            }
        })
        .await
        .map_err(|e| format!("Error interno al abrir historia local: {}", e))
        .and_then(|res| res);

        let final_result = match local_result {
            Ok(Some(data)) => Ok(data),
            Ok(None) => match wp_opt {
                Some(wp) => api::cargar_historia_para_lectura_api(&wp, &story_id).await,
                None => Ok((String::new(), Vec::new())),
            },
            Err(err) => Err(err),
        };

        match final_result {
            Ok((resolved_title, chapters)) if !chapters.is_empty() => {
                let id_progreso = story_id.clone();
                let saved_index =
                    tokio::task::spawn_blocking(move || db::obtener_progreso(&id_progreso))
                        .await
                        .ok()
                        .flatten()
                        .unwrap_or(0);
                let initial_index = saved_index.min(chapters.len() - 1);

                if let Ok(mut state) = state_arc.lock() {
                    state.opened_story_id = story_id.clone();
                    state.opened_story_title = resolved_title;
                    state.opened_chapters = chapters;
                    state.current_chapter_index = initial_index;

                    if let Some(app) = app_weak.upgrade() {
                        actualizar_capitulo_en_ui(&app, &state);
                        app.set_is_loading(false);
                    }
                }
            }
            Ok(_) => {
                if let Some(app) = app_weak.upgrade() {
                    app.set_is_loading(false);
                    app.set_error_message(
                        "Esta historia aún no tiene capítulos disponibles.".into(),
                    );
                }
            }
            Err(err) => {
                eprintln!("Error cargando capítulos: {}", err);
                if let Some(app) = app_weak.upgrade() {
                    app.set_is_loading(false);
                    app.set_error_message(err.into());
                }
            }
        }
    }))
    .unwrap();
}

fn limpiar_datos_ui_sesion(app: &AppWindow) {
    app.set_is_logged_in(false);
    app.set_is_reading(false);
    app.set_current_username("".into());
    app.set_profile_full_name("".into());
    app.set_profile_avatar(slint::Image::default());
    app.set_profile_followers(0);
    app.set_profile_following(0);
    app.set_profile_stories_count(0);
    app.set_profile_description("".into());
    app.set_home_stories(slint::ModelRc::default());
    app.set_library_stories(slint::ModelRc::default());
    app.set_search_results(slint::ModelRc::default());
    app.set_draft_status_message("".into());
    app.set_reader_story_title("".into());
    app.set_reader_chapter_title("".into());
    app.set_reader_chapter_index(0);
    app.set_reader_total_chapters(0);
    app.set_current_tab(0);
    app.set_error_message("".into());
}

fn main() -> Result<(), slint::PlatformError> {
    if let Err(e) = db::inicializar_db() {
        eprintln!("Advertencia al inicializar wattpad.db: {}", e);
    }

    let app = AppWindow::new()?;
    let tokio_rt =
        tokio::runtime::Runtime::new().expect("No se pudo inicializar el runtime de Tokio");
    let _tokio_guard = tokio_rt.enter();
    let app_state = Arc::new(Mutex::new(AppState::new()));

    // Sincronizar estado inicial con la UI
    {
        let state = app_state.lock().unwrap();
        app.set_is_logged_in(state.is_logged_in);
        app.set_current_username(state.username.clone().into());
        app.set_current_tab(state.current_tab);
        app.set_is_offline(state.is_offline);
        app.set_is_reading(false);
        app.set_home_stories(slint::ModelRc::default());
        app.set_library_stories(slint::ModelRc::default());
    }

    // 1. Cargar sesión (wp_client, reqwest_client, cookie_store, tiene_token_en_disco)
    let (wp_client, http_client, cookie_store, tiene_sesion) = sesion::cargar_cliente();
    let wp_client = Arc::new(wp_client);
    let http_client = Arc::new(http_client);

    let app_handle = app.as_weak();
    cargar_destacados_home(app_handle.clone(), Arc::clone(&app_state), Arc::clone(&wp_client));

    // Si hay una cookie `token` guardada, verificamos con Wattpad si sigue vigente.
    if tiene_sesion {
        app.set_is_loading(true);
        let app_weak = app_handle.clone();
        let state_verify = Arc::clone(&app_state);
        let wp_verify = Arc::clone(&wp_client);
        let http_verify = Arc::clone(&http_client);
        let store_verify = Arc::clone(&cookie_store);

        slint::spawn_local(Compat::new(async move {
            match sesion::verificar_sesion(&http_verify).await {
                Ok(username) => {
                    println!("Sesión previa válida para @{}", username);
                    if let Ok(mut state) = state_verify.lock() {
                        state.is_logged_in = true;
                        state.username = username.clone();
                        state.session_salt = crypto::generar_salt();
                        state.is_offline = false;
                    }
                    if let Some(app) = app_weak.upgrade() {
                        app.set_current_username(username.into());
                        app.set_is_logged_in(true);
                        app.set_is_offline(false);
                        app.set_error_message("".into());
                        app.set_is_loading(false);
                    }
                    cargar_datos_usuario(app_weak, state_verify, wp_verify);
                }
                Err(err) => {
                    eprintln!("La sesión guardada ya no es válida: {}", err);
                    sesion::cerrar_sesion(&http_verify, &store_verify).await;
                    if let Ok(mut state) = state_verify.lock() {
                        state.is_logged_in = false;
                        state.username.clear();
                        state.password.clear();
                        state.session_salt = [0u8; crypto::SALT_LEN];
                        state.user_profile = None;
                        state.library_stories.clear();
                    }
                    if let Some(app) = app_weak.upgrade() {
                        limpiar_datos_ui_sesion(&app);
                        app.set_is_loading(false);
                    }
                }
            }
        }))
        .unwrap();
    }

    // --- Navegación (Bottom Navigation Bar) ---
    let app_handle_nav = app_handle.clone();
    let state_nav = Arc::clone(&app_state);

    app.on_tab_selected(move |tab_index| {
        if let Ok(mut state) = state_nav.lock() {
            state.current_tab = tab_index;
        }
        if let Some(app) = app_handle_nav.upgrade() {
            app.set_current_tab(tab_index);
            app.set_error_message("".into());
            app.set_draft_status_message("".into());
        }
    });

    // --- Home: abrir historia destacada ---
    let app_handle_home = app_handle.clone();
    let state_home = Arc::clone(&app_state);
    let wp_home = Arc::clone(&wp_client);
    app.on_home_story_clicked(move |story_id| {
        abrir_historia_para_lectura(
            app_handle_home.clone(),
            Arc::clone(&state_home),
            Some(Arc::clone(&wp_home)),
            story_id.to_string(),
        );
    });

    // --- Crear: guardar borrador local ---
    let app_handle_draft = app_handle.clone();
    app.on_save_draft_requested(move |title, content| {
        let app_weak = app_handle_draft.clone();
        let titulo = title.to_string();
        let cuerpo = content.to_string();

        if titulo.trim().is_empty() && cuerpo.trim().is_empty() {
            if let Some(app) = app_weak.upgrade() {
                app.set_draft_status_message(
                    "Escribe un título o contenido antes de guardar.".into(),
                );
            }
            return;
        }

        if let Some(app) = app_weak.upgrade() {
            app.set_is_loading(true);
            app.set_draft_status_message("".into());
        }

        slint::spawn_local(Compat::new(async move {
            let texto_archivo = if titulo.trim().is_empty() {
                cuerpo
            } else {
                format!("Título: {}\n\n{}", titulo.trim(), cuerpo)
            };

            let res = tokio::task::spawn_blocking(move || {
                std::fs::write("borrador_historia.txt", texto_archivo)
                    .map_err(|e| format!("Error al guardar borrador: {}", e))
            })
            .await
            .map_err(|e| format!("Error interno al guardar borrador: {}", e))
            .and_then(|r| r);

            if let Some(app) = app_weak.upgrade() {
                app.set_is_loading(false);
                match res {
                    Ok(()) => app.set_draft_status_message(
                        "Borrador guardado en borrador_historia.txt".into(),
                    ),
                    Err(err) => app.set_draft_status_message(err.into()),
                }
            }
        }))
        .unwrap();
    });

    // --- Login ---
    let app_handle_login = app_handle.clone();
    let state_login = Arc::clone(&app_state);
    let wp_login = Arc::clone(&wp_client);
    let http_login = Arc::clone(&http_client);
    let cookie_store_login = Arc::clone(&cookie_store);

    app.on_login_requested(move |username, password| {
        let app_weak = app_handle_login.clone();
        let state_arc = Arc::clone(&state_login);
        let wp = Arc::clone(&wp_login);
        let http = Arc::clone(&http_login);
        let cookie_store_arc = Arc::clone(&cookie_store_login);
        let user = username.to_string();
        let pass = password.to_string();

        if let Some(app) = app_weak.upgrade() {
            app.set_is_loading(true);
            app.set_error_message("".into());
        }

        slint::spawn_local(Compat::new(async move {
            match sesion::iniciar_sesion(&http, &cookie_store_arc, &user, &pass).await {
                Ok(info) => {
                    println!("Login exitoso como @{}", info.username);
                    if let Ok(mut state) = state_arc.lock() {
                        state.is_logged_in = true;
                        state.username = info.username.clone();
                        state.password = pass;
                        state.session_salt = crypto::generar_salt();
                        state.current_tab = 0;
                        state.is_offline = false;
                    }
                    if let Some(app) = app_weak.upgrade() {
                        app.set_current_username(info.username.into());
                        app.set_password("".into());
                        app.set_error_message("".into());
                        app.set_current_tab(0);
                        app.set_is_offline(false);
                        app.set_is_logged_in(true);
                        app.set_is_loading(false);
                    }
                    cargar_datos_usuario(app_weak, state_arc, wp);
                }
                Err(msg) => {
                    eprintln!("Error de autenticación: {}", msg);
                    if let Some(app) = app_weak.upgrade() {
                        app.set_error_message(msg.into());
                        app.set_is_loading(false);
                    }
                }
            }
        }))
        .unwrap();
    });

    // --- Logout ---
    let app_handle_logout = app_handle.clone();
    let state_logout = Arc::clone(&app_state);
    let http_logout = Arc::clone(&http_client);
    let cookie_store_logout = Arc::clone(&cookie_store);

    app.on_logout_requested(move || {
        let app_weak = app_handle_logout.clone();
        let state_arc = Arc::clone(&state_logout);
        let http = Arc::clone(&http_logout);
        let store = Arc::clone(&cookie_store_logout);

        if let Some(app) = app_weak.upgrade() {
            app.set_is_loading(true);
        }

        slint::spawn_local(Compat::new(async move {
            sesion::cerrar_sesion(&http, &store).await;
            if let Ok(mut state) = state_arc.lock() {
                state.is_logged_in = false;
                state.username.clear();
                state.password.clear();
                state.session_salt = [0u8; crypto::SALT_LEN];
                state.current_tab = 0;
                state.user_profile = None;
                state.library_stories.clear();
                state.opened_story_id.clear();
                state.opened_story_title.clear();
                state.opened_chapters.clear();
                state.current_chapter_index = 0;
            }
            if let Some(app) = app_weak.upgrade() {
                limpiar_datos_ui_sesion(&app);
                app.set_is_loading(false);
            }
        }))
        .unwrap();
    });

    // --- Búsqueda ---
    let app_handle_search = app_handle.clone();
    let wp_search = Arc::clone(&wp_client);

    app.on_search_requested(move |query| {
        let app_weak = app_handle_search.clone();
        let wp = Arc::clone(&wp_search);
        let q = query.to_string();

        if q.trim().is_empty() {
            return;
        }

        if let Some(app) = app_weak.upgrade() {
            app.set_is_loading(true);
            app.set_error_message("".into());
        }

        slint::spawn_local(Compat::new(async move {
            match api::buscar_historias(&q, &wp).await {
                Ok(stories) => {
                    let titulos: Vec<slint::SharedString> = if stories.is_empty() {
                        vec![slint::SharedString::from("No se encontraron historias.")]
                    } else {
                        stories
                            .into_iter()
                            .map(slint::SharedString::from)
                            .collect()
                    };

                    if let Some(app) = app_weak.upgrade() {
                        app.set_search_results(slint::ModelRc::new(slint::VecModel::from(titulos)));
                        app.set_is_loading(false);
                    }
                }
                Err(msg) => {
                    eprintln!("Error en búsqueda: {}", msg);
                    if let Some(app) = app_weak.upgrade() {
                        app.set_error_message(msg.into());
                        app.set_is_loading(false);
                    }
                }
            }
        }))
        .unwrap();
    });

    // --- Descarga de historia completa (EPUB cifrado) para lectura offline ---
    let app_handle_download = app_handle.clone();
    let state_download = Arc::clone(&app_state);
    let wp_download = Arc::clone(&wp_client);

    app.on_download_story_requested(move |story_id| {
        let app_weak = app_handle_download.clone();
        let state_arc = Arc::clone(&state_download);
        let wp = Arc::clone(&wp_download);
        let id = story_id.to_string();

        if let Some(app) = app_weak.upgrade() {
            app.set_is_loading(true);
            app.set_error_message("".into());
        }

        slint::spawn_local(Compat::new(async move {
            let (username, password, salt) = {
                let mut state = state_arc.lock().unwrap();
                if state.session_salt == [0u8; crypto::SALT_LEN] {
                    state.session_salt = crypto::generar_salt();
                }
                (
                    state.username.clone(),
                    state.password.clone(),
                    state.session_salt,
                )
            };

            match api::descargar_historia_completa(&wp, &id, &username, &password, &salt).await {
                Ok(num_capitulos) => {
                    println!("Historia {} descargada y encriptada ({} capítulos).", id, num_capitulos);
                    if let Some(app) = app_weak.upgrade() {
                        let model = app.get_library_stories();
                        let updated: Vec<LibraryStoryItem> = model
                            .iter()
                            .map(|mut item| {
                                if item.id.as_str() == id {
                                    item.is_downloaded = true;
                                }
                                item
                            })
                            .collect();
                        app.set_library_stories(slint::ModelRc::new(slint::VecModel::from(
                            updated,
                        )));
                        app.set_is_loading(false);
                    }
                }
                Err(err) => {
                    eprintln!("Error descargando historia {}: {}", id, err);
                    if let Some(app) = app_weak.upgrade() {
                        app.set_error_message(err.into());
                        app.set_is_loading(false);
                    }
                }
            }
        }))
        .unwrap();
    });

    // --- Lectura offline desde EPUB cifrado (con respaldo SQLite/API) ---
    let app_handle_read = app_handle.clone();
    let state_read = Arc::clone(&app_state);
    let wp_read = Arc::clone(&wp_client);

    app.on_read_story_requested(move |story_id| {
        abrir_historia_para_lectura(
            app_handle_read.clone(),
            Arc::clone(&state_read),
            Some(Arc::clone(&wp_read)),
            story_id.to_string(),
        );
    });

    // --- Controles del lector ---
    let app_handle_close = app_handle.clone();
    app.on_close_reader_requested(move || {
        if let Some(app) = app_handle_close.upgrade() {
            app.set_is_reading(false);
        }
    });

    let app_handle_prev = app_handle.clone();
    let state_prev = Arc::clone(&app_state);
    app.on_prev_chapter_requested(move || {
        if let Ok(mut state) = state_prev.lock() {
            if state.current_chapter_index > 0 {
                state.current_chapter_index -= 1;
                if let Some(app) = app_handle_prev.upgrade() {
                    actualizar_capitulo_en_ui(&app, &state);
                }
            }
        }
    });

    let app_handle_next = app_handle.clone();
    let state_next = Arc::clone(&app_state);
    app.on_next_chapter_requested(move || {
        if let Ok(mut state) = state_next.lock() {
            if state.current_chapter_index + 1 < state.opened_chapters.len() {
                state.current_chapter_index += 1;
                if let Some(app) = app_handle_next.upgrade() {
                    actualizar_capitulo_en_ui(&app, &state);
                }
            }
        }
    });

    // --- Notificaciones en segundo plano ---
    tokio::spawn(notifier::iniciar_notificaciones(Arc::clone(&wp_client)));

    app.run()
}
