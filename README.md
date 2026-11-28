# mi-cliente-wattpad

Cliente de escritorio no oficial de Wattpad para Linux, construido con **Rust** y **Slint**, con lector nativo basado en WebView (`wry` / WebKitGTK), soporte de lectura sin conexión con almacenamiento cifrado e historial de progreso en SQLite.

---

## Características principales

- **Inicio y Destacados:** Exploración de historias destacadas con portadas en caché local (y datos de respaldo cuando no hay conexión).
- **Búsqueda:** Búsqueda de historias en Wattpad con indicador visual de carga (`Spinner`).
- **Biblioteca y Descargas Offline:** Listado de la biblioteca del usuario, descarga completa de historias para lectura sin conexión e indicador visual de progreso.
- **Perfil de usuario:** Visualización de avatar, nombre completo, biografía, seguidores, seguidos e historias publicadas, con persistencia de sesión mediante cookies.
- **Lector nativo en WebView:** Ventana independiente con `wry` (WebKitGTK) que muestra los capítulos formateados con tema oscuro, barra inferior fija y navegación **Anterior / Siguiente** mediante IPC y actualización dinámica por JavaScript con retorno automático del scroll al inicio.
- **Descargas cifradas localmente:** Los capítulos descargados para lectura offline se serializan y cifran localmente con **ChaCha20-Poly1305** y clave derivada por SHA-256 (`username`, `password` y `salt`).
- **Historial de lectura (Seguir leyendo):** Registro persistente del último capítulo leído por historia en SQLite (`wattpad.db`, tabla `progress`), retomando automáticamente la lectura donde la dejaste.
- **Notificaciones en segundo plano:** Comprobación periódica de cambios en votos, capítulos o historias seguidas mediante `notify-rust`.
- **Borrador local:** Pestaña _Crear_ para redactar y guardar borradores locales en `borrador_historia.txt`.

---

## Stack Tecnológico

- **Lenguaje:** Rust (Edition 2021)
- **Interfaz gráfica:** [Slint](https://slint.dev/) (`1.19`)
- **Cliente de Wattpad y red:** `wp-mini` (`0.2.0-alpha.3`), `reqwest` (`0.13`), `reqwest_cookie_store`, `cookie_store`
- **Motor del lector web:** `wry` (`0.39`) sobre `gtk` (`0.18`) / WebKitGTK
- **Base de datos local:** SQLite embebido mediante `rusqlite` (`0.31`, feature `bundled`)
- **Criptografía:** `chacha20poly1305` (`0.10`), `sha2` (`0.10`), `rand` (`0.8`)
- **Concurrencia y utilidades:** `tokio`, `async-compat`, `serde`, `serde_json`, `notify-rust`

---

## ⚠️ ADVERTENCIA IMPORTANTE (Disclaimer)

- **API no oficial:** Wattpad **no** dispone de una API pública oficial para clientes de terceros. Este proyecto interactúa con endpoints internos y no documentados (a través de `wp-mini` y peticiones HTTP directas), lo que significa que **la aplicación puede dejar de funcionar total o parcialmente en cualquier momento** si Wattpad cambia sus rutas, respuestas o mecanismos de autenticación.
- **Descargas cifradas y DRM casero:** Las historias descargadas para lectura sin conexión se guardan cifradas localmente en disco (un mecanismo tipo _DRM casero_ basado en ChaCha20-Poly1305) con el objetivo de evitar la distribución directa de obras en texto plano o EPUB sin protección. Aun así, **el código fuente del proyecto es abierto**, por lo que este cifrado es una medida disuasoria local y no una protección criptográfica invulnerable frente a quien tenga acceso al entorno de ejecución.
- **Uso bajo tu propia responsabilidad:** Este software es un proyecto independiente, sin afiliación, respaldo ni relación oficial con Wattpad. Respeta los términos de servicio de la plataforma y los derechos de autor de los escritores.

---

## Requisitos del Sistema

Este proyecto está orientado principalmente a **Linux** (en concreto probado y configurado para entorno GTK/WebKitGTK). En sistemas distintos a Linux el lector WebView GTK no está habilitado.

### Arch Linux (y derivadas como Manjaro / EndeavourOS)

Necesitarás el toolchain de Rust y las librerías nativas de GTK 3, WebKit2GTK 4.1, SSL y compilación C:

```bash
sudo pacman -S --needed rustup base-devel pkgconf openssl gtk3 webkit2gtk-4.1
```

> Si usas `rustup` por primera vez, inicializa el toolchain estable con `rustup default stable`.

### Debian / Ubuntu (referencia equivalente)

```bash
sudo apt update && sudo apt install -y build-essential pkg-config libssl-dev libgtk-3-dev libwebkit2gtk-4.1-dev
```

_(Además requiere un servidor gráfico X11 o Wayland para Slint y la ventana WebView, y un daemon de notificaciones D-Bus si deseas recibir notificaciones de escritorio.)_

---

## Instalación y Compilación

1. Clona el repositorio y entra al directorio del proyecto:

```bash
git clone https://github.com/ZoroTheDark/wattpad-desktop.git
cd wattpad
```

1. Compila en modo release:

```bash
cargo build --release
```

1. Ejecuta el binario compilado:

```bash
./target/release/mi-cliente-wattpad
```

Para ejecutar las pruebas unitarias:

```bash
cargo test
```

---

## Cómo contribuir

Las contribuciones son bienvenidas, especialmente para mantener la compatibilidad con cambios en Wattpad, mejorar la interfaz en Slint o pulir el lector WebView:

1. Haz un _fork_ del repositorio.
2. Crea una rama descriptiva para tu cambio (`git checkout -b feat/mi-mejora`).
3. Asegúrate de que el código compile y pase las pruebas (`cargo check && cargo test`).
4. Abre un _Pull Request_ explicando claramente el problema resuelto o la mejora añadida.

---

## Licencia

Este proyecto se distribuye bajo la licencia **MIT** (consulta o añade un archivo `LICENSE` en la raíz del repositorio si deseas incluir el texto legal completo).
