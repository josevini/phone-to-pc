//! Starting the app with the session: an XDG autostart entry that runs it in the tray only (`--hidden`).

use std::io;
use std::path::{Path, PathBuf};

/// The app's entry in `$XDG_CONFIG_HOME/autostart`.
pub struct Autostart {
    entry: PathBuf,
}

impl Autostart {
    /// `config` is the XDG config home, such as `~/.config`.
    pub fn new(config: &Path) -> Autostart {
        Autostart { entry: config.join("autostart/clipsync-desktop.desktop") }
    }

    /// In `$XDG_CONFIG_HOME`, or `~/.config` when it is unset or relative; `None` without an absolute `HOME`.
    pub fn from_env() -> Option<Autostart> {
        Autostart::from_vars(|name| std::env::var_os(name).map(PathBuf::from))
    }

    /// As [`Autostart::from_env`], with the environment read through `var`.
    pub fn from_vars(var: impl Fn(&str) -> Option<PathBuf>) -> Option<Autostart> {
        let absolute = |name: &str| var(name).filter(|p| p.is_absolute());
        let config = absolute("XDG_CONFIG_HOME").or_else(|| absolute("HOME").map(|home| home.join(".config")))?;
        Some(Autostart::new(&config))
    }

    pub fn enabled(&self) -> bool {
        self.entry.exists()
    }

    /// Starts `program` (this app) with the session, or stops doing so.
    pub fn set(&self, on: bool, program: &Path) -> io::Result<()> {
        if !on {
            return match std::fs::remove_file(&self.entry) {
                Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
                other => other,
            };
        }
        if let Some(dir) = self.entry.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let entry = format!(
            "[Desktop Entry]\nType=Application\nName=clipsync\nComment=The clipsync tray icon\nExec={}\nIcon=clipsync\n\
             Terminal=false\nX-GNOME-Autostart-enabled=true\n",
            exec(program)
        );
        std::fs::write(&self.entry, entry)
    }
}

/// The `Exec` value that runs `program --hidden`: the path quoted, with the characters the Desktop Entry
/// specification reserves escaped, then every backslash escaped again as the value's own escape.
fn exec(program: &Path) -> String {
    let mut quoted = String::from("\"");
    for c in program.to_string_lossy().chars() {
        if matches!(c, '"' | '`' | '$' | '\\') {
            quoted.push('\\');
        }
        quoted.push(c);
    }
    quoted.push('"');
    format!("{} --hidden", quoted.replace('\\', "\\\\"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turning_it_on_writes_an_entry_that_starts_the_app_hidden() {
        let tmp = tempfile::tempdir().unwrap();
        let autostart = Autostart::new(tmp.path());
        assert!(!autostart.enabled());

        autostart.set(true, Path::new("/usr/bin/clipsync-desktop")).unwrap();
        assert!(autostart.enabled());
        let entry = std::fs::read_to_string(tmp.path().join("autostart/clipsync-desktop.desktop")).unwrap();
        assert!(entry.starts_with("[Desktop Entry]\nType=Application\n"), "{entry}");
        assert!(entry.contains("\nExec=\"/usr/bin/clipsync-desktop\" --hidden\n"), "{entry}");
        assert!(entry.contains("\nIcon=clipsync\n"), "{entry}");

        autostart.set(false, Path::new("/usr/bin/clipsync-desktop")).unwrap();
        assert!(!autostart.enabled());
        autostart.set(false, Path::new("/usr/bin/clipsync-desktop")).unwrap();
    }

    #[test]
    fn the_entry_goes_in_the_xdg_config_home_or_under_home() {
        let vars = |pairs: &'static [(&'static str, &'static str)]| {
            move |name: &str| pairs.iter().find(|(n, _)| *n == name).map(|(_, v)| PathBuf::from(v))
        };
        let entry = |pairs| Autostart::from_vars(vars(pairs)).map(|a| a.entry);
        let expected = |dir: &str| Some(PathBuf::from(dir).join("autostart/clipsync-desktop.desktop"));
        assert_eq!(entry(&[("XDG_CONFIG_HOME", "/x/config"), ("HOME", "/home/me")]), expected("/x/config"));
        assert_eq!(entry(&[("HOME", "/home/me")]), expected("/home/me/.config"));
        // The XDG spec ignores a relative XDG_CONFIG_HOME.
        assert_eq!(entry(&[("XDG_CONFIG_HOME", "config"), ("HOME", "/home/me")]), expected("/home/me/.config"));
        assert_eq!(entry(&[]), None);
    }

    #[test]
    fn the_program_path_is_quoted_for_the_desktop_entry() {
        assert_eq!(
            exec(Path::new("/home/me/My Apps/clipsync-desktop")),
            "\"/home/me/My Apps/clipsync-desktop\" --hidden"
        );
        assert_eq!(exec(Path::new("/opt/a\"b$c`d\\e")), "\"/opt/a\\\\\"b\\\\$c\\\\`d\\\\\\\\e\" --hidden");
    }
}
