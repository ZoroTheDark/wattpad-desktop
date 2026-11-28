// src/lector_web.rs
use crate::db::{self, DbChapter};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::{mpsc, OnceLock};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ReaderPrefs {
    pub font_size: u32,
    pub theme: String,
    pub font_family: String,
}

impl Default for ReaderPrefs {
    fn default() -> Self {
        Self {
            font_size: 16,
            theme: "theme-light".into(),
            font_family: "sans-serif".into(),
        }
    }
}

fn reader_prefs_path() -> PathBuf {
    PathBuf::from("reader_prefs.json")
}

pub fn cargar_preferencias_lector(is_dark_mode: bool) -> ReaderPrefs {
    let path = reader_prefs_path();
    if path.exists() {
        if let Some(prefs) = fs::read_to_string(path)
            .ok()
            .and_then(|json| serde_json::from_str::<ReaderPrefs>(&json).ok())
        {
            return normalizar_preferencias(prefs, is_dark_mode);
        }
    }
    let mut prefs = ReaderPrefs::default();
    if is_dark_mode {
        prefs.theme = "theme-dark".into();
    }
    prefs
}

fn guardar_preferencias_lector(prefs: &ReaderPrefs) {
    if let Ok(json) = serde_json::to_vec_pretty(prefs) {
        if let Err(err) = fs::write(reader_prefs_path(), json) {
            eprintln!("No se pudieron guardar las preferencias del lector: {err}");
        }
    }
}

const CSS_LECTOR: &str = r#"
<style>
  :root {
    color-scheme: light dark;
  }
  body {
    font-family: sans-serif;
    padding: 40px 40px 96px 40px;
    line-height: 1.6;
    max-width: 800px;
    margin: auto;
    background-color: #121212;
    color: #e0e0e0;
    font-size: 18px;
    word-wrap: break-word;
  }
  p {
    margin: 0 0 1.15em 0;
  }
  body.theme-light { background-color: #ffffff !important; color: #000000 !important; }
  body.theme-sepia { background-color: #f4ecd8 !important; color: #5b4636 !important; }
  body.theme-dark { background-color: #121212 !important; color: #e0e0e0 !important; }
  body.theme-light a { color: #0645ad; }
  body.theme-sepia a { color: #805a36; }
  body.theme-dark a { color: #66b2ff; }
  .lector-settings-button { position: fixed; top: 14px; right: 16px; z-index: 1100; border: 0; border-radius: 8px; padding: 10px 14px; background: #ff7846; color: #fff; font-size: 16px; cursor: pointer; }
  .lector-settings { display: none; position: fixed; top: 58px; right: 16px; z-index: 1100; width: 230px; padding: 14px; border: 1px solid #777; border-radius: 10px; background: #fff; color: #222; box-shadow: 0 4px 18px #0005; font: 14px sans-serif; }
  .lector-settings.open { display: block; }
  .lector-settings section { margin: 0 0 10px; }
  .lector-settings strong { display: block; margin-bottom: 5px; }
  .lector-settings button { margin: 2px; padding: 6px 9px; cursor: pointer; }
  img {
    max-width: 100%;
    height: auto;
    display: block;
    margin: 1.2em auto;
    border-radius: 6px;
  }
  a {
    color: #66b2ff;
  }
  .barra-navegacion {
    position: fixed;
    left: 0;
    right: 0;
    bottom: 0;
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 12px;
    padding: 12px 24px;
    background-color: rgba(18, 18, 18, 0.95);
    border-top: 1px solid #2d2d2d;
    backdrop-filter: blur(8px);
    z-index: 1000;
  }
  .indicador-capitulo {
    font-size: 14px;
    color: #9a9a9a;
  }
  .btn-nav {
    background-color: #ff7846;
    color: #ffffff;
    border: none;
    border-radius: 8px;
    padding: 10px 20px;
    font-size: 15px;
    font-weight: 600;
    cursor: pointer;
  }
  .btn-nav:hover:not(:disabled) {
    background-color: #ff8c61;
  }
  .btn-nav:disabled {
    opacity: 0.4;
    cursor: not-allowed;
    background-color: #2d2d2d;
  }
</style>
"#;

#[derive(Debug, Clone)]
struct EstadoLector {
    story_id: String,
    story_title: String,
    capitulos: Vec<DbChapter>,
    indice_actual: usize,
    is_dark: bool,
    preferencias: ReaderPrefs,
}

enum SolicitudWebView {
    Lector(EstadoLector),
    Notificaciones,
}

fn ruta_datos_webview() -> PathBuf {
    if let Ok(data_home) = std::env::var("XDG_DATA_HOME") {
        if !data_home.trim().is_empty() {
            return PathBuf::from(data_home)
                .join("mi-cliente-wattpad")
                .join("wattpad_webview_data");
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        if !home.trim().is_empty() {
            return PathBuf::from(home)
                .join(".local/share/mi-cliente-wattpad/wattpad_webview_data");
        }
    }
    PathBuf::from("wattpad_webview_data")
}

fn escapar_html_titulo(texto: &str) -> String {
    let mut salida = String::with_capacity(texto.len());
    for ch in texto.chars() {
        match ch {
            '&' => salida.push_str("&amp;"),
            '<' => salida.push_str("&lt;"),
            '>' => salida.push_str("&gt;"),
            '"' => salida.push_str("&quot;"),
            _ => salida.push(ch),
        }
    }
    salida
}

fn construir_barra_navegacion(indice: usize, total: usize) -> String {
    let disabled_anterior = if indice == 0 { " disabled" } else { "" };
    let disabled_siguiente = if indice + 1 >= total { " disabled" } else { "" };
    let capitulo_visible = if total == 0 { 0 } else { indice + 1 };

    let html = r#"
<nav class="barra-navegacion" aria-label="Navegación de capítulos">
  <button id="btn-anterior" class="btn-nav" type="button"__PREV_DISABLED__>Anterior</button>
  <span class="indicador-capitulo">Capítulo __CHAPTER__ de __TOTAL__</span>
  <button id="btn-siguiente" class="btn-nav" type="button"__NEXT_DISABLED__>Siguiente</button>
</nav>
<button class="lector-settings-button" id="lector-settings-toggle" type="button" aria-label="Preferencias de lectura">Aa</button>
<aside class="lector-settings" id="lector-settings" aria-label="Preferencias de lectura">
  <section><strong>Tamaño</strong><button type="button" data-size="-1">A-</button><button type="button" data-size="1">A+</button></section>
  <section><strong>Fuente</strong><button type="button" data-font="sans-serif">Sans</button><button type="button" data-font="serif">Serif</button><button type="button" data-font="monospace">Mono</button></section>
  <section><strong>Tema</strong><button type="button" data-theme="light">Claro</button><button type="button" data-theme="sepia">Sepia</button><button type="button" data-theme="dark">Oscuro</button></section>
</aside>
<script>
  (function () {
    var body = document.body;
    var settings = document.getElementById('lector-settings');
    var toggle = document.getElementById('lector-settings-toggle');
    var defaults = { size: '18px', font: 'sans-serif', theme: body.classList.contains('theme-dark') ? 'dark' : 'light' };
    function read(key) { try { return localStorage.getItem('wattpad-reader-' + key); } catch (_) { return null; } }
    function save(key, value) { try { localStorage.setItem('wattpad-reader-' + key, value); } catch (_) {} }
    function apply() {
      var size = parseInt(read('size'), 10);
      body.style.fontSize = (isFinite(size) ? Math.max(12, Math.min(36, size)) : parseInt(defaults.size, 10)) + 'px';
      body.style.fontFamily = read('font') || defaults.font;
      body.classList.remove('theme-light', 'theme-sepia', 'theme-dark');
      body.classList.add('theme-' + (read('theme') || defaults.theme));
    }
    apply();
    toggle.addEventListener('click', function () { settings.classList.toggle('open'); });
    settings.addEventListener('click', function (event) {
      var button = event.target.closest('button');
      if (!button) return;
      if (button.dataset.size) {
        var current = parseInt(body.style.fontSize, 10) || 18;
        var size = Math.max(12, Math.min(36, current + parseInt(button.dataset.size, 10) * 2));
        body.style.fontSize = size + 'px'; save('size', size);
      }
      if (button.dataset.font) { body.style.fontFamily = button.dataset.font; save('font', button.dataset.font); }
      if (button.dataset.theme) { body.classList.remove('theme-light', 'theme-sepia', 'theme-dark'); body.classList.add('theme-' + button.dataset.theme); save('theme', button.dataset.theme); }
      window.ipc.postMessage('save_prefs:' + JSON.stringify({
        font_size: parseInt(body.style.fontSize, 10) || 16,
        theme: ['theme-light','theme-sepia','theme-dark'].find(function (name) { return body.classList.contains(name); }) || 'theme-light',
        font_family: body.style.fontFamily.replace(/['\"]/g, '') || 'sans-serif'
      }));
    });
  })();
  window.scrollTo(0, 0);
  document.getElementById('btn-anterior')?.addEventListener('click', function () {
    window.ipc.postMessage('anterior');
  });
  document.getElementById('btn-siguiente')?.addEventListener('click', function () {
    window.ipc.postMessage('siguiente');
  });
</script>
"#;
    html.replace("__PREV_DISABLED__", disabled_anterior)
        .replace("__CHAPTER__", &capitulo_visible.to_string())
        .replace("__TOTAL__", &total.to_string())
        .replace("__NEXT_DISABLED__", disabled_siguiente)
}

fn css_lector(is_dark: bool) -> String {
    if is_dark {
        return format!(
            "{}<style>body {{ background-color: #121212; color: #e0e0e0; }} a {{ color: #66b2ff; }}</style>",
            CSS_LECTOR
        );
    }

    let css_claro = CSS_LECTOR
        .replace("#121212", "#ffffff")
        .replace("#e0e0e0", "#222222")
        .replace("#66b2ff", "#0645ad")
        .replace("rgba(18, 18, 18, 0.95)", "rgba(255, 255, 255, 0.95)")
        .replace("#2d2d2d", "#dddddd")
        .replace("#9a9a9a", "#666666");
    format!(
        "{}<style>body {{ background-color: #ffffff; color: #222222; }} a {{ color: #0645ad; }}</style>",
        css_claro
    )
}

fn normalizar_preferencias(mut prefs: ReaderPrefs, is_dark: bool) -> ReaderPrefs {
    prefs.font_size = prefs.font_size.clamp(12, 36);
    if !matches!(
        prefs.theme.as_str(),
        "theme-light" | "theme-sepia" | "theme-dark"
    ) {
        prefs.theme = if is_dark { "theme-dark" } else { "theme-light" }.into();
    }
    if !matches!(
        prefs.font_family.as_str(),
        "sans-serif" | "serif" | "monospace"
    ) {
        prefs.font_family = "sans-serif".into();
    }
    prefs
}

fn construir_html_completo(
    titulo: &str,
    html: &str,
    indice: usize,
    total: usize,
    is_dark: bool,
    preferencias: &ReaderPrefs,
) -> String {
    let barra = construir_barra_navegacion(indice, total);
    let css = css_lector(is_dark);
    let titulo_escapado = escapar_html_titulo(titulo);
    let prefs = normalizar_preferencias(preferencias.clone(), is_dark);
    let tema = prefs.theme;
    format!(
        "<!DOCTYPE html><html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1.0\"><title>{}</title>{}<style>body {{ font-size: {}px; font-family: {}; }}</style></head><body class=\"{}\"><div id=\"chapter-content\"><h1>{}</h1>{}</div>{}</body></html>",
        titulo_escapado, css, prefs.font_size, prefs.font_family, tema, titulo_escapado, extraer_contenido_capitulo(html), barra
    )
}

fn formatear_titulo_ventana(story_title: &str, chapter_title: &str) -> String {
    let titulo = if story_title.trim().is_empty() {
        chapter_title.trim().to_string()
    } else if chapter_title.trim().is_empty() {
        story_title.trim().to_string()
    } else {
        format!("{} — {}", story_title.trim(), chapter_title.trim())
    };

    if titulo.is_empty() {
        "Lector Wattpad".to_string()
    } else {
        titulo
    }
}

fn construir_vista_capitulo(estado: &EstadoLector) -> Option<(String, String)> {
    let chapter = estado.capitulos.get(estado.indice_actual)?;
    let titulo = formatear_titulo_ventana(&estado.story_title, &chapter.title);
    let html = construir_html_completo(
        &titulo,
        &chapter.content,
        estado.indice_actual,
        estado.capitulos.len(),
        estado.is_dark,
        &estado.preferencias,
    );
    Some((titulo, html))
}

fn extraer_contenido_capitulo(html: &str) -> &str {
    let lower = html.to_ascii_lowercase();
    if let Some(body_start) = lower.find("<body") {
        if let Some(start) = lower[body_start..].find('>') {
            let content_start = body_start + start + 1;
            if let Some(end) = lower[content_start..].find("</body>") {
                return &html[content_start..content_start + end];
            }
        }
    }
    html
}

fn construir_script_actualizacion_capitulo(
    contenido: &str,
    titulo: &str,
    indice: usize,
    total: usize,
) -> Result<String, serde_json::Error> {
    let contenido = format!(
        "<h1>{}</h1>{}",
        escapar_html_titulo(titulo),
        extraer_contenido_capitulo(contenido)
    );
    let contenido_serializado = serde_json::to_string(&contenido)?
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029");
    let capitulo = if total == 0 { 0 } else { indice + 1 };
    let anterior_disabled = indice == 0;
    let siguiente_disabled = indice + 1 >= total;
    Ok(format!(
        "document.getElementById('chapter-content').innerHTML = {contenido_serializado};\nvar prev = document.getElementById('btn-anterior'); var next = document.getElementById('btn-siguiente');\nif (prev) prev.disabled = {anterior_disabled}; if (next) next.disabled = {siguiente_disabled};\nvar indicator = document.querySelector('.indicador-capitulo'); if (indicator) indicator.textContent = 'Capítulo {capitulo} de {total}';\n(function () {{ var b = document.body; try {{ var size = localStorage.getItem('wattpad-reader-size'); var font = localStorage.getItem('wattpad-reader-font'); var theme = localStorage.getItem('wattpad-reader-theme'); if (size) b.style.fontSize = size + 'px'; if (font) b.style.fontFamily = font; if (theme) {{ b.classList.remove('theme-light','theme-sepia','theme-dark'); b.classList.add('theme-' + theme); }} }} catch (_) {{}} }})();\nwindow.scrollTo(0, 0);"
    ))
}

#[cfg(target_os = "linux")]
fn actualizar_html_en_webview(
    webview: &wry::WebView,
    contenido: &str,
    titulo: &str,
    indice: usize,
    total: usize,
) {
    match construir_script_actualizacion_capitulo(contenido, titulo, indice, total) {
        Ok(script) => {
            if let Err(err) = webview.evaluate_script(&script) {
                eprintln!("Error actualizando el capítulo en WebView: {}", err);
            }
        }
        Err(err) => {
            eprintln!("Error serializando HTML para el WebView: {}", err);
        }
    }
}

#[cfg(target_os = "linux")]
fn iniciar_hilo_lector() -> &'static mpsc::Sender<SolicitudWebView> {
    use gtk::prelude::*;
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::time::Duration;
    use wry::WebViewBuilderExtUnix;

    static LECTOR_TX: OnceLock<mpsc::Sender<SolicitudWebView>> = OnceLock::new();

    LECTOR_TX.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<SolicitudWebView>();

        std::thread::spawn(move || {
            if let Err(err) = gtk::init() {
                eprintln!("No se pudo inicializar GTK para el lector web: {}", err);
                return;
            }

            let ventana_activa: Rc<RefCell<Option<(gtk::Window, wry::WebView)>>> =
                Rc::new(RefCell::new(None));
            let ventana_notificaciones: Rc<RefCell<Option<(gtk::Window, wry::WebView)>>> =
                Rc::new(RefCell::new(None));
            let estado_activo: Rc<RefCell<Option<EstadoLector>>> = Rc::new(RefCell::new(None));

            let ruta_datos = ruta_datos_webview();
            if let Some(parent) = ruta_datos.parent() {
                if let Err(err) = std::fs::create_dir_all(parent) {
                    eprintln!("No se pudo crear el directorio de datos de Wry: {err}");
                }
            }
            let mut web_context = wry::WebContext::new(Some(ruta_datos));

            let ventana_ref = Rc::clone(&ventana_activa);
            let ventana_notificaciones_ref = Rc::clone(&ventana_notificaciones);
            let estado_ref = Rc::clone(&estado_activo);

            gtk::glib::timeout_add_local(Duration::from_millis(50), move || {
                while let Ok(solicitud) = rx.try_recv() {
                    if matches!(&solicitud, SolicitudWebView::Notificaciones) {
                        if let Some((prev_win, _)) = ventana_notificaciones_ref.borrow_mut().take()
                        {
                            prev_win.close();
                        }
                        let window = gtk::Window::new(gtk::WindowType::Toplevel);
                        window.set_title("Notificaciones de Wattpad");
                        window.set_default_size(1000, 720);
                        let vbox = gtk::Box::new(gtk::Orientation::Vertical, 0);
                        window.add(&vbox);
                        match wry::WebViewBuilder::new_gtk(&vbox)
                            .with_web_context(&mut web_context)
                            .build()
                        {
                            Ok(webview) => {
                                if let Err(err) =
                                    webview.load_url("https://www.wattpad.com/notifications")
                                {
                                    eprintln!(
                                        "No se pudo abrir las notificaciones de Wattpad: {err}"
                                    );
                                    window.close();
                                    continue;
                                }
                                let close_ref = Rc::clone(&ventana_notificaciones_ref);
                                window.connect_delete_event(move |_, _| {
                                    close_ref.borrow_mut().take();
                                    gtk::glib::Propagation::Proceed
                                });
                                window.show_all();
                                *ventana_notificaciones_ref.borrow_mut() = Some((window, webview));
                            }
                            Err(err) => {
                                eprintln!("Error creando WebView de notificaciones: {err}");
                                window.close();
                            }
                        }
                        continue;
                    }

                    let SolicitudWebView::Lector(nuevo_estado) = solicitud else {
                        continue;
                    };
                    let Some((titulo, html_completo)) = construir_vista_capitulo(&nuevo_estado)
                    else {
                        continue;
                    };

                    if let Some((prev_win, _)) = ventana_ref.borrow_mut().take() {
                        prev_win.close();
                    }
                    *estado_ref.borrow_mut() = Some(nuevo_estado);

                    let window = gtk::Window::new(gtk::WindowType::Toplevel);
                    window.set_title(&titulo);
                    window.set_default_size(800, 600);

                    let vbox = gtk::Box::new(gtk::Orientation::Vertical, 0);
                    window.add(&vbox);

                    let ventana_ipc = Rc::clone(&ventana_ref);
                    let estado_ipc = Rc::clone(&estado_ref);

                    match wry::WebViewBuilder::new_gtk(&vbox)
                        .with_web_context(&mut web_context)
                        .with_html(&html_completo)
                        .with_ipc_handler(move |req| {
                            let accion = req.body().trim();
                            if let Some(json) = accion.strip_prefix("save_prefs:") {
                                if let Ok(mut prefs) = serde_json::from_str::<ReaderPrefs>(json) {
                                    if let Ok(mut estado_opt) = estado_ipc.try_borrow_mut() {
                                        if let Some(estado) = estado_opt.as_mut() {
                                            prefs = normalizar_preferencias(prefs, estado.is_dark);
                                            guardar_preferencias_lector(&prefs);
                                            estado.preferencias = prefs;
                                        }
                                    }
                                }
                                return;
                            }
                            let siguiente_vista = if let Ok(mut estado_opt) =
                                estado_ipc.try_borrow_mut()
                            {
                                if let Some(estado) = estado_opt.as_mut() {
                                    let cambio = match accion {
                                        "siguiente"
                                            if estado.indice_actual + 1
                                                < estado.capitulos.len() =>
                                        {
                                            true
                                        }
                                        "anterior" if estado.indice_actual > 0 => true,
                                        _ => false,
                                    };
                                    if cambio {
                                        if accion == "siguiente" {
                                            estado.indice_actual += 1;
                                        }
                                        if accion == "anterior" {
                                            estado.indice_actual -= 1;
                                        }
                                        let id_progreso = if estado.story_id.trim().is_empty() {
                                            estado
                                                .capitulos
                                                .get(estado.indice_actual)
                                                .map(|c| c.story_id.clone())
                                                .unwrap_or_default()
                                        } else {
                                            estado.story_id.clone()
                                        };
                                        if !id_progreso.trim().is_empty() {
                                            if let Err(err) = db::guardar_progreso(
                                                &id_progreso,
                                                estado.indice_actual,
                                            ) {
                                                eprintln!(
                                                    "Error guardando progreso de lectura: {}",
                                                    err
                                                );
                                            }
                                        }
                                        estado.capitulos.get(estado.indice_actual).map(|chapter| {
                                            (
                                                formatear_titulo_ventana(
                                                    &estado.story_title,
                                                    &chapter.title,
                                                ),
                                                chapter.content.clone(),
                                                estado.indice_actual,
                                                estado.capitulos.len(),
                                            )
                                        })
                                    } else {
                                        None
                                    }
                                } else {
                                    None
                                }
                            } else {
                                None
                            };

                            if let Some((nuevo_titulo, nuevo_contenido, indice, total)) =
                                siguiente_vista
                            {
                                if let Ok(ventana_opt) = ventana_ipc.try_borrow() {
                                    if let Some((ref win, ref wv)) = *ventana_opt {
                                        win.set_title(&nuevo_titulo);
                                        actualizar_html_en_webview(
                                            wv,
                                            &nuevo_contenido,
                                            &nuevo_titulo,
                                            indice,
                                            total,
                                        );
                                    }
                                }
                            }
                        })
                        .build()
                    {
                        Ok(webview) => {
                            let ventana_cierre = Rc::clone(&ventana_ref);
                            let estado_cierre = Rc::clone(&estado_ref);
                            window.connect_delete_event(move |_, _| {
                                ventana_cierre.borrow_mut().take();
                                estado_cierre.borrow_mut().take();
                                gtk::glib::Propagation::Proceed
                            });
                            window.show_all();
                            *ventana_ref.borrow_mut() = Some((window, webview));
                        }
                        Err(err) => {
                            eprintln!("Error creando WebView de lectura: {}", err);
                            estado_ref.borrow_mut().take();
                            window.close();
                        }
                    }
                }
                gtk::glib::ControlFlow::Continue
            });

            gtk::main();
        });

        tx
    })
}

/// Abre una ventana nativa con WebView (`wry`) con navegación IPC entre capítulos.
pub fn abrir_lector(
    story_id: String,
    story_title: String,
    mut capitulos: Vec<DbChapter>,
    indice_actual: usize,
    is_dark: bool,
    preferencias: ReaderPrefs,
) {
    if capitulos.is_empty() && !story_id.trim().is_empty() {
        if let Ok(conn) = db::inicializar_db() {
            if let Ok(cargados) = db::cargar_capitulos_historia(&conn, &story_id) {
                capitulos = cargados;
            }
        }
    }

    if capitulos.is_empty() {
        eprintln!("No hay capítulos disponibles para abrir en el lector web.");
        return;
    }

    let indice_valido = indice_actual.min(capitulos.len() - 1);
    let estado = EstadoLector {
        story_id,
        story_title,
        capitulos,
        indice_actual: indice_valido,
        is_dark,
        preferencias: normalizar_preferencias(preferencias, is_dark),
    };

    #[cfg(target_os = "linux")]
    {
        if let Err(err) = iniciar_hilo_lector().send(SolicitudWebView::Lector(estado)) {
            eprintln!("No se pudo enviar el capítulo al lector web: {}", err);
        }
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = estado;
        eprintln!("El lector web GTK solo está habilitado en Linux.");
    }
}

/// Abre las notificaciones de Wattpad en un WebView que comparte el contexto y las cookies del lector.
pub fn abrir_notificaciones() {
    #[cfg(target_os = "linux")]
    if let Err(err) = iniciar_hilo_lector().send(SolicitudWebView::Notificaciones) {
        eprintln!("No se pudo abrir la ventana de notificaciones: {err}");
    }

    #[cfg(not(target_os = "linux"))]
    eprintln!("El WebView de notificaciones GTK solo está habilitado en Linux.");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inyecta_barra_navegacion_y_script_ipc() {
        let html =
            construir_html_completo("Cap 1", "<p>Texto</p>", 0, 3, true, &ReaderPrefs::default());
        assert!(html.contains("window.ipc.postMessage('siguiente')"));
        assert!(html.contains("window.ipc.postMessage('anterior')"));
        assert!(html.contains("window.scrollTo(0, 0);"));
        assert!(html.contains("id=\"btn-anterior\" class=\"btn-nav\" type=\"button\" disabled"));
        assert!(html.contains("Capítulo 1 de 3"));
    }

    #[test]
    fn construye_script_actualizacion_sin_reemplazar_dom() {
        let script =
            construir_script_actualizacion_capitulo("<p>Línea 1\nLínea 2</p>", "Cap \"2\"", 1, 3)
                .unwrap();
        assert!(script.contains("document.getElementById('chapter-content').innerHTML"));
        assert!(!script.contains("document.write("));
        assert!(script.contains("wattpad-reader-font"));
        assert!(script.contains("wattpad-reader-size"));
        assert!(script.contains("wattpad-reader-theme"));
        assert!(script.contains("window.scrollTo(0, 0);"));
        let html =
            construir_html_completo("Cap 1", "<p>Texto</p>", 0, 3, true, &ReaderPrefs::default());
        assert!(html.contains("<body class=\"theme-dark\">"));
        assert!(html.contains("id=\"chapter-content\""));
    }
}
