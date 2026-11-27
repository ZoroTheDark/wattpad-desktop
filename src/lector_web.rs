// src/lector_web.rs
use crate::db::{self, DbChapter};
use std::sync::{mpsc, OnceLock};

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
    background-color: #18181b;
    color: #f4f4f5;
    font-size: 18px;
    word-wrap: break-word;
  }
  p {
    margin: 0 0 1.15em 0;
  }
  img {
    max-width: 100%;
    height: auto;
    display: block;
    margin: 1.2em auto;
    border-radius: 6px;
  }
  a {
    color: #ff7846;
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
    background-color: rgba(24, 24, 27, 0.95);
    border-top: 1px solid #3f3f46;
    backdrop-filter: blur(8px);
    z-index: 1000;
  }
  .indicador-capitulo {
    font-size: 14px;
    color: #a1a1aa;
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
    background-color: #3f3f46;
  }
</style>
"#;

#[derive(Debug, Clone)]
struct EstadoLector {
    story_id: String,
    story_title: String,
    capitulos: Vec<DbChapter>,
    indice_actual: usize,
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

    format!(
        r#"
<nav class="barra-navegacion" aria-label="Navegación de capítulos">
  <button id="btn-anterior" class="btn-nav" type="button"{}>Anterior</button>
  <span class="indicador-capitulo">Capítulo {} de {}</span>
  <button id="btn-siguiente" class="btn-nav" type="button"{}>Siguiente</button>
</nav>
<script>
  window.scrollTo(0, 0);
  document.getElementById('btn-anterior')?.addEventListener('click', function () {{
    window.ipc.postMessage('anterior');
  }});
  document.getElementById('btn-siguiente')?.addEventListener('click', function () {{
    window.ipc.postMessage('siguiente');
  }});
</script>
"#,
        disabled_anterior, capitulo_visible, total, disabled_siguiente
    )
}

fn inyectar_antes_de_cierre_body(html: &str, fragmento: &str) -> String {
    let lower = html.to_ascii_lowercase();
    if let Some(pos) = lower.rfind("</body>") {
        let mut out = String::with_capacity(html.len() + fragmento.len());
        out.push_str(&html[..pos]);
        out.push_str(fragmento);
        out.push_str(&html[pos..]);
        out
    } else {
        let mut out = String::with_capacity(html.len() + fragmento.len());
        out.push_str(html);
        out.push_str(fragmento);
        out
    }
}

fn construir_html_completo(titulo: &str, html: &str, indice: usize, total: usize) -> String {
    let barra = construir_barra_navegacion(indice, total);
    let lower = html.to_ascii_lowercase();

    if let Some(pos) = lower.find("</head>") {
        let mut con_css = String::with_capacity(html.len() + CSS_LECTOR.len());
        con_css.push_str(&html[..pos]);
        con_css.push_str(CSS_LECTOR);
        con_css.push_str(&html[pos..]);
        return inyectar_antes_de_cierre_body(&con_css, &barra);
    }

    if lower.contains("<html") {
        if let Some(pos) = lower.find("<body") {
            let mut con_css = String::with_capacity(html.len() + CSS_LECTOR.len() + 36);
            con_css.push_str(&html[..pos]);
            con_css.push_str("<head><meta charset=\"utf-8\">");
            con_css.push_str(CSS_LECTOR);
            con_css.push_str("</head>");
            con_css.push_str(&html[pos..]);
            return inyectar_antes_de_cierre_body(&con_css, &barra);
        }
    }

    let titulo_escapado = escapar_html_titulo(titulo);
    format!(
        "<!DOCTYPE html><html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1.0\"><title>{}</title>{}</head><body><h1>{}</h1>{}{}</body></html>",
        titulo_escapado, CSS_LECTOR, titulo_escapado, html, barra
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
    );
    Some((titulo, html))
}

fn construir_script_reemplazo_html(html_completo: &str) -> Result<String, serde_json::Error> {
    let html_serializado = serde_json::to_string(html_completo)?
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029");
    Ok(format!(
        "document.open();\ndocument.write({html_serializado});\ndocument.close();\nwindow.scrollTo(0, 0);\nif (document.documentElement) {{ document.documentElement.scrollTop = 0; }}\nif (document.body) {{ document.body.scrollTop = 0; }}\nrequestAnimationFrame(function() {{ window.scrollTo(0, 0); }});"
    ))
}

#[cfg(target_os = "linux")]
fn actualizar_html_en_webview(webview: &wry::WebView, html_completo: &str) {
    match construir_script_reemplazo_html(html_completo) {
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
fn iniciar_hilo_lector() -> &'static mpsc::Sender<EstadoLector> {
    use gtk::prelude::*;
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::time::Duration;
    use wry::WebViewBuilderExtUnix;

    static LECTOR_TX: OnceLock<mpsc::Sender<EstadoLector>> = OnceLock::new();

    LECTOR_TX.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<EstadoLector>();

        std::thread::spawn(move || {
            if let Err(err) = gtk::init() {
                eprintln!("No se pudo inicializar GTK para el lector web: {}", err);
                return;
            }

            let ventana_activa: Rc<RefCell<Option<(gtk::Window, wry::WebView)>>> =
                Rc::new(RefCell::new(None));
            let estado_activo: Rc<RefCell<Option<EstadoLector>>> = Rc::new(RefCell::new(None));

            let ventana_ref = Rc::clone(&ventana_activa);
            let estado_ref = Rc::clone(&estado_activo);

            gtk::glib::timeout_add_local(Duration::from_millis(50), move || {
                while let Ok(nuevo_estado) = rx.try_recv() {
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
                        .with_html(&html_completo)
                        .with_ipc_handler(move |req| {
                            let accion = req.body().trim();
                            let siguiente_vista = if let Ok(mut estado_opt) =
                                estado_ipc.try_borrow_mut()
                            {
                                if let Some(estado) = estado_opt.as_mut() {
                                    let cambio = match accion {
                                        "siguiente"
                                            if estado.indice_actual + 1 < estado.capitulos.len() =>
                                        {
                                            estado.indice_actual += 1;
                                            true
                                        }
                                        "anterior" if estado.indice_actual > 0 => {
                                            estado.indice_actual -= 1;
                                            true
                                        }
                                        _ => false,
                                    };
                                    if cambio {
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
                                            if let Err(err) =
                                                db::guardar_progreso(&id_progreso, estado.indice_actual)
                                            {
                                                eprintln!(
                                                    "Error guardando progreso de lectura: {}",
                                                    err
                                                );
                                            }
                                        }
                                        construir_vista_capitulo(estado)
                                    } else {
                                        None
                                    }
                                } else {
                                    None
                                }
                            } else {
                                None
                            };

                            if let Some((nuevo_titulo, nuevo_html)) = siguiente_vista {
                                if let Ok(ventana_opt) = ventana_ipc.try_borrow() {
                                    if let Some((ref win, ref wv)) = *ventana_opt {
                                        win.set_title(&nuevo_titulo);
                                        actualizar_html_en_webview(wv, &nuevo_html);
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
    };

    #[cfg(target_os = "linux")]
    {
        if let Err(err) = iniciar_hilo_lector().send(estado) {
            eprintln!("No se pudo enviar el capítulo al lector web: {}", err);
        }
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = estado;
        eprintln!("El lector web GTK solo está habilitado en Linux.");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inyecta_barra_navegacion_y_script_ipc() {
        let html = construir_html_completo("Cap 1", "<p>Texto</p>", 0, 3);
        assert!(html.contains("window.ipc.postMessage('siguiente')"));
        assert!(html.contains("window.ipc.postMessage('anterior')"));
        assert!(html.contains("window.scrollTo(0, 0);"));
        assert!(html.contains("id=\"btn-anterior\" class=\"btn-nav\" type=\"button\" disabled"));
        assert!(html.contains("Capítulo 1 de 3"));
    }

    #[test]
    fn construye_script_reemplazo_html_escapado() {
        let html = construir_html_completo("Cap \"2\"", "<p>Línea 1\nLínea 2</p>", 1, 3);
        let script = construir_script_reemplazo_html(&html).unwrap();
        assert!(script.contains("document.open();"));
        assert!(script.contains("document.write("));
        assert!(script.contains("document.close();"));
        assert!(script.contains("window.scrollTo(0, 0);"));
        assert!(script.contains("window.ipc.postMessage('siguiente')"));
        assert!(script.contains("window.ipc.postMessage('anterior')"));
    }
}
