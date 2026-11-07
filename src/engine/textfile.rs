//! Écriture des fichiers texte comme Python en mode texte (fins de ligne CRLF sous Windows).

pub fn write_text(path: &std::path::Path, s: &str) -> std::io::Result<()> {
    if cfg!(windows) {
        std::fs::write(path, s.replace('\n', "\r\n"))
    } else {
        std::fs::write(path, s)
    }
}
