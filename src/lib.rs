//! # canaima-updater
//!
//! Módulo de verificación y actualización automática de aplicaciones para Canaima GNU/Linux.
//! Provee una ventana nativa GTK con el diseño y paleta oficial de Canaima, y ejecuta
//! la actualización con elevación de privilegios de forma segura usando PolicyKit (`pkexec`).

use std::process::Command;
#[cfg(target_os = "linux")]
use std::sync::{Arc, Mutex};
#[cfg(target_os = "linux")]
use std::thread;

/// Configuración para el verificador de actualizaciones.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UpdaterConfig {
    /// Nombre del paquete en el sistema Debian / Canaima (ej: "mi-app-canaima").
    pub package_name: String,
    /// Nombre comercial o visible de la aplicación (ej: "Mi Aplicación Canaima").
    pub product_name: String,
    /// Versión actualmente en ejecución (ej: "1.0.0").
    pub current_version: String,
    /// URL remota opcional para consultar versión (ej: endpoint JSON).
    /// Si es None, consulta el gestor de paquetes APT local (`apt-cache policy`).
    pub check_url: Option<String>,
    /// Iniciar ventana en modo oscuro.
    pub dark_mode: bool,
}

impl UpdaterConfig {
    pub fn new(package_name: impl Into<String>, product_name: impl Into<String>, current_version: impl Into<String>) -> Self {
        Self {
            package_name: package_name.into(),
            product_name: product_name.into(),
            current_version: current_version.into(),
            check_url: None,
            dark_mode: false,
        }
    }

    pub fn with_dark_mode(mut self, dark_mode: bool) -> Self {
        self.dark_mode = dark_mode;
        self
    }

    pub fn with_check_url(mut self, url: impl Into<String>) -> Self {
        self.check_url = Some(url.into());
        self
    }
}

/// Valida estrictamente que el nombre de paquete cumpla la política de Debian
/// (mínimo 2 caracteres, inicia con alfanumérico en minúsculas y solo contiene [a-z0-9+.-])
/// para prevenir ataques de inyección de comandos o argumentos maliciosos.
pub fn is_valid_debian_package_name(name: &str) -> bool {
    if name.len() < 2 || name.len() > 100 {
        return false;
    }
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_lowercase() || c.is_ascii_digit() => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '+' || c == '-' || c == '.')
}

/// Consulta si existe una versión más reciente en el repositorio APT del sistema.
pub fn check_apt_update(package_name: &str) -> Result<Option<String>, String> {
    if !is_valid_debian_package_name(package_name) {
        return Err(format!("Nombre de paquete inválido: '{}'", package_name));
    }

    let output = Command::new("apt-cache")
        .arg("policy")
        .arg(package_name)
        .output()
        .map_err(|e| format!("Error al ejecutar apt-cache: {}", e))?;

    if !output.status.success() {
        return Err("apt-cache finalizó con código de error".to_string());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut installed = String::new();
    let mut candidate = String::new();

    for line in stdout.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("Instalados:") || trimmed.starts_with("Installed:") {
            if let Some(val) = trimmed.split(':').nth(1) {
                installed = val.trim().to_string();
            }
        } else if trimmed.starts_with("Candidato:") || trimmed.starts_with("Candidate:") {
            if let Some(val) = trimmed.split(':').nth(1) {
                candidate = val.trim().to_string();
            }
        }
    }

    if candidate.is_empty() || candidate == "(none)" || candidate == "(ninguno)" {
        return Ok(None);
    }

    if !installed.is_empty() && installed != "(none)" && installed != "(ninguno)" && installed != candidate {
        return Ok(Some(candidate));
    }

    Ok(None)
}

/// Ejecuta la verificación y muestra la ventana de actualización si hay una nueva versión.
pub fn check_and_prompt_update(config: UpdaterConfig) {
    #[cfg(target_os = "linux")]
    {
        let cfg = config.clone();
        thread::spawn(move || {
            // Verificar si hay actualización disponible
            let new_version = match check_apt_update(&cfg.package_name) {
                Ok(Some(v)) => v,
                _ => return, // No hay actualización o error silencioso al verificar
            };

            if gtk::is_initialized() {
                // Si GTK ya está activo (ej. proceso Tauri), despachamos la creación en el hilo principal
                glib::idle_add_once(move || {
                    show_update_dialog_gtk(&cfg, &new_version, false);
                });
            } else {
                // Modo independiente (ej. CLI o subproceso sin bucle GTK previo)
                show_update_dialog_gtk(&cfg, &new_version, true);
            }
        });
    }
}

/// Muestra la ventana modal nativa GTK con estética Canaima.
#[cfg(target_os = "linux")]
pub fn show_update_dialog_gtk(config: &UpdaterConfig, new_version: &str, run_main_loop: bool) {
    use gtk::prelude::*;

    if run_main_loop && gtk::init().is_err() {
        eprintln!("Error: no fue posible inicializar GTK para la ventana de actualización.");
        return;
    }

    let window = gtk::Window::new(gtk::WindowType::Toplevel);
    window.set_title("Actualización del Sistema - Canaima");
    window.set_default_size(440, 260);
    window.set_position(gtk::WindowPosition::Center);
    window.set_resizable(false);

    // Contenedor principal
    let main_box = gtk::Box::new(gtk::Orientation::Vertical, 0);

    // HeaderBar superior
    let header_bar = gtk::HeaderBar::new();
    header_bar.set_show_close_button(true);
    header_bar.set_title(Some("Actualización disponible"));
    header_bar.style_context().add_class("canaima-updater-header");
    if config.dark_mode {
        header_bar.style_context().add_class("is-dark");
    }
    window.set_titlebar(Some(&header_bar));

    // Contenido
    let content_box = gtk::Box::new(gtk::Orientation::Vertical, 12);
    content_box.set_margin_start(24);
    content_box.set_margin_end(24);
    content_box.set_margin_top(20);
    content_box.set_margin_bottom(20);

    // Título y detalles
    let title_label = gtk::Label::new(None);
    title_label.set_markup(&format!(
        "<span size='large' weight='bold' color='#0b6793'>¡Nueva versión para {}!</span>",
        glib::markup_escape_text(&config.product_name)
    ));
    title_label.set_xalign(0.0);

    let desc_label = gtk::Label::new(None);
    desc_label.set_markup(&format!(
        "Versión actual: <b>{}</b>\nNueva versión: <span color='#0284c7'><b>{}</b></span>\n\n¿Deseas descargar e instalar esta actualización del repositorio oficial?",
        glib::markup_escape_text(&config.current_version),
        glib::markup_escape_text(new_version)
    ));
    desc_label.set_xalign(0.0);
    desc_label.set_line_wrap(true);

    // Spinner de carga y estado
    let spinner_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let spinner = gtk::Spinner::new();
    let status_label = gtk::Label::new(None);
    status_label.set_xalign(0.0);
    spinner_box.pack_start(&spinner, false, false, 0);
    spinner_box.pack_start(&status_label, true, true, 0);
    spinner_box.set_visible(false);

    // Botones de acción
    let button_box = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    button_box.set_halign(gtk::Align::End);
    button_box.set_margin_top(12);

    let btn_cancel = gtk::Button::with_label("Recordar más tarde");
    btn_cancel.style_context().add_class("btn-secondary");

    let btn_update = gtk::Button::with_label("Actualizar ahora");
    btn_update.style_context().add_class("btn-primary");

    button_box.pack_start(&btn_cancel, false, false, 0);
    button_box.pack_start(&btn_update, false, false, 0);

    content_box.pack_start(&title_label, false, false, 0);
    content_box.pack_start(&desc_label, false, false, 0);
    content_box.pack_start(&spinner_box, false, false, 0);
    content_box.pack_start(&button_box, false, false, 0);

    main_box.pack_start(&content_box, true, true, 0);
    window.add(&main_box);

    // Estilos CSS Canaima
    let provider = gtk::CssProvider::new();
    let css = "
        .canaima-updater-header {
            background-color: #0b6793;
            color: #ffffff;
            border-bottom: 2px solid #084c6e;
        }
        .canaima-updater-header.is-dark {
            background-color: #063952;
            color: #ffffff;
            border-bottom: 1px solid #000000;
        }
        button.btn-primary {
            background-color: #0b6793;
            color: white;
            border-radius: 6px;
            padding: 6px 16px;
            font-weight: bold;
            border: none;
        }
        button.btn-primary:hover {
            background-color: #0284c7;
        }
        button.btn-secondary {
            background-color: #e2e8f0;
            color: #334155;
            border-radius: 6px;
            padding: 6px 14px;
            border: 1px solid #cbd5e1;
        }
        button.btn-secondary:hover {
            background-color: #cbd5e1;
        }
    ";
    let _ = provider.load_from_data(css.as_bytes());
    if let Some(screen) = gtk::gdk::Screen::default() {
        gtk::StyleContext::add_provider_for_screen(
            &screen,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }

    let win_cancel = window.clone();
    btn_cancel.connect_clicked(move |_| {
        win_cancel.close();
    });

    let pkg_name = config.package_name.clone();
    let win_update = window.clone();
    let is_running = Arc::new(Mutex::new(false));

    let (sender, receiver) = {
        #[allow(deprecated)]
        glib::MainContext::channel(glib::Priority::default())
    };

    let s_spin = spinner.clone();
    let s_lbl = status_label.clone();
    let w_done = win_update;
    let b_upd = btn_update.clone();
    let b_can_rx = btn_cancel.clone();
    let is_running_rx = is_running.clone();

    receiver.attach(None, move |res: Result<(), String>| {
        s_spin.stop();
        match res {
            Ok(_) => {
                s_lbl.set_markup("<span color='green'><b>¡Actualización completada! Reinicia la app.</b></span>");
                let _ = Command::new("notify-send")
                    .arg("Canaima GNU/Linux")
                    .arg("La aplicación fue actualizada con éxito.")
                    .spawn();
                let w_close = w_done.clone();
                glib::timeout_add_seconds_local(3, move || {
                    w_close.close();
                    glib::ControlFlow::Break
                });
            }
            Err(err) => {
                s_lbl.set_markup(&format!(
                    "<span color='red'><b>Error:</b> {}</span>",
                    glib::markup_escape_text(&err)
                ));
                b_upd.set_sensitive(true);
                b_can_rx.set_sensitive(true);
                if let Ok(mut r) = is_running_rx.lock() {
                    *r = false;
                }
            }
        }
        glib::ControlFlow::Break
    });

    let b_can_click = btn_cancel;
    btn_update.connect_clicked(move |b| {
        let mut running = match is_running.lock() {
            Ok(guard) => guard,
            Err(_) => return,
        };
        if *running {
            return;
        }
        *running = true;

        b.set_sensitive(false);
        b_can_click.set_sensitive(false);
        spinner_box.set_visible(true);
        spinner.start();
        status_label.set_markup("<i>Ejecutando actualización con PolicyKit (pkexec)...</i>");

        let pkg = pkg_name.clone();
        let tx = sender.clone();

        thread::spawn(move || {
            let res = execute_pkexec_update(&pkg);
            let _ = tx.send(res);
        });
    });

    if run_main_loop {
        window.connect_destroy(|_| {
            gtk::main_quit();
        });
        window.show_all();
        gtk::main();
    } else {
        window.show_all();
    }
}

/// Ejecuta la actualización del paquete mediante `pkexec` de forma segura.
///
/// Invoca directamente el ejecutable del sistema sin pasar por un shell `sh -c`
/// para prevenir ataques de inyección de comandos.
pub fn execute_pkexec_update(package_name: &str) -> Result<(), String> {
    if !is_valid_debian_package_name(package_name) {
        return Err(format!("Validación fallida para el paquete '{}'", package_name));
    }

    // Ejecuta `pkexec apt-get install --only-upgrade -y <paquete>`
    let status = Command::new("pkexec")
        .arg("apt-get")
        .arg("install")
        .arg("--only-upgrade")
        .arg("-y")
        .arg(package_name)
        .status()
        .map_err(|e| format!("Error al invocar pkexec: {}", e))?;

    if status.success() {
        Ok(())
    } else {
        match status.code() {
            Some(126) | Some(127) => Err("Autenticación cancelada por el usuario.".to_string()),
            Some(code) => Err(format!("Falló la instalación (código de salida: {})", code)),
            None => Err("El proceso de actualización fue interrumpido.".to_string()),
        }
    }
}

pub mod commands {
    use super::*;

    /// Comando de Tauri para comprobar actualizaciones desde el Frontend (Vue 3 / TypeScript).
    #[tauri::command]
    pub fn check_for_updates(package_name: String) -> Result<Option<String>, String> {
        check_apt_update(&package_name)
    }

    /// Comando de Tauri para ejecutar actualización con PolicyKit desde el Frontend.
    #[tauri::command]
    pub fn run_pkexec_update(package_name: String) -> Result<(), String> {
        execute_pkexec_update(&package_name)
    }
}

pub use commands::{check_for_updates, run_pkexec_update};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_debian_package_names() {
        assert!(is_valid_debian_package_name("canaima-updater"));
        assert!(is_valid_debian_package_name("libgtk-3-0"));
        assert!(is_valid_debian_package_name("python3.11"));
        assert!(is_valid_debian_package_name("g++"));
    }

    #[test]
    fn test_invalid_debian_package_names() {
        assert!(!is_valid_debian_package_name(""));
        assert!(!is_valid_debian_package_name("a")); // demasiado corto (< 2)
        assert!(!is_valid_debian_package_name("CanaimaUpdater")); // mayúsculas
        assert!(!is_valid_debian_package_name("-bad-start")); // inicia con guion
        assert!(!is_valid_debian_package_name("pkg;rm -rf /")); // inyección de comandos
        assert!(!is_valid_debian_package_name("pkg$(whoami)")); // caracteres prohibidos
    }

    #[test]
    fn test_updater_config() {
        let cfg = UpdaterConfig::new("mi-app", "Mi App", "1.0.0")
            .with_dark_mode(true)
            .with_check_url("https://example.com/check");
        assert_eq!(cfg.package_name, "mi-app");
        assert_eq!(cfg.product_name, "Mi App");
        assert_eq!(cfg.current_version, "1.0.0");
        assert!(cfg.dark_mode);
        assert_eq!(cfg.check_url.as_deref(), Some("https://example.com/check"));
    }
}
