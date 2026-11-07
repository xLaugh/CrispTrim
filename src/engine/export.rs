//! Exports : FCPXML (DaVinci Resolve, Final Cut Pro), XML Premiere Pro, EDL Vegas Pro, MP4 monté, JSON.

use super::cutter::CutResult;
use super::media::{tool, MediaInfo};
use super::transcribe::Word;
use super::Fps;
use serde::Serialize;
use std::path::Path;
use std::process::Stdio;

pub type Frames = [(i64, i64)];

// --------------------------------------------------------------------------- XML minimal

struct El {
    tag: &'static str,
    attrs: Vec<(&'static str, String)>,
    text: Option<String>,
    children: Vec<El>,
}

impl El {
    fn new(tag: &'static str) -> Self {
        El { tag, attrs: Vec::new(), text: None, children: Vec::new() }
    }
    fn attr(mut self, k: &'static str, v: impl ToString) -> Self {
        self.attrs.push((k, v.to_string()));
        self
    }
    fn text(tag: &'static str, v: impl ToString) -> Self {
        let mut e = El::new(tag);
        e.text = Some(v.to_string());
        e
    }
    fn push(&mut self, child: El) -> &mut El {
        self.children.push(child);
        self.children.last_mut().unwrap()
    }
    fn add(mut self, child: El) -> Self {
        self.children.push(child);
        self
    }

    fn write(&self, out: &mut String, depth: usize) {
        let pad = "  ".repeat(depth);
        out.push_str(&pad);
        out.push('<');
        out.push_str(self.tag);
        for (k, v) in &self.attrs {
            out.push_str(&format!(" {k}=\"{}\"", esc_attr(v)));
        }
        if self.children.is_empty() {
            match &self.text {
                Some(t) => out.push_str(&format!(">{}</{}>", esc_text(t), self.tag)),
                None => out.push_str(" />"),
            }
            return;
        }
        out.push('>');
        for c in &self.children {
            out.push('\n');
            c.write(out, depth + 1);
        }
        out.push('\n');
        out.push_str(&pad);
        out.push_str(&format!("</{}>", self.tag));
    }
}

fn esc_text(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn esc_attr(s: &str) -> String {
    esc_text(s).replace('"', "&quot;").replace('\n', "&#10;").replace('\r', "&#13;").replace('\t', "&#09;")
}

fn pretty(root: &El, doctype: &str) -> String {
    let mut body = String::new();
    root.write(&mut body, 0);
    format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE {doctype}>\n{body}\n")
}

/// `urllib.parse.quote` avec « / » gardé.
fn quote(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"_.-~/".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn as_posix(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

/// `Path.as_uri()` : file:///C:/dossier/fichier%20x.mp4
fn as_uri(p: &Path) -> String {
    let s = as_posix(p);
    if let Some(rest) = s.strip_prefix("//") {
        return format!("file://{}", quote(rest));
    }
    if s.len() >= 2 && s.as_bytes()[1] == b':' {
        return format!("file:///{}{}", &s[..2], quote(&s[2..]));
    }
    format!("file://{}", quote(&s))
}

fn stem(p: &Path) -> String {
    p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

fn file_name(p: &Path) -> String {
    p.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

// --------------------------------------------------------------------------- FCPXML

/// Temps rationnel FCPXML, multiple exact de la durée d'une image.
fn ftime(frames: i64, fps: Fps) -> String {
    if frames == 0 {
        return "0s".into();
    }
    if fps.den != 1 || frames % fps.num != 0 {
        format!("{}/{}s", frames * fps.den, fps.num)
    } else {
        format!("{}s", frames / fps.num)
    }
}

pub fn fcpxml(info: &MediaInfo, frames: &Frames, name: &str) -> String {
    let fps = info.fps;
    let total = info.total_frames();
    let tl_len: i64 = frames.iter().map(|(a, b)| b - a).sum();
    let st = stem(&info.path);

    let format = El::new("format")
        .attr("id", "r1")
        .attr("frameDuration", format!("{}/{}s", fps.den, fps.num))
        .attr("width", info.width)
        .attr("height", info.height);
    let asset = El::new("asset")
        .attr("id", "r2")
        .attr("name", &st)
        .attr("start", "0s")
        .attr("duration", ftime(total, fps))
        .attr("hasVideo", if info.has_video { "1" } else { "0" })
        .attr("hasAudio", if info.has_audio { "1" } else { "0" })
        .attr("format", "r1")
        .attr("audioSources", "1")
        .attr("audioChannels", info.audio_channels.max(1))
        .attr("audioRate", info.audio_rate)
        .add(El::new("media-rep").attr("kind", "original-media").attr("src", as_uri(&info.path)));

    let mut spine = El::new("spine");
    let mut offset = 0;
    for &(a, b) in frames {
        spine.push(
            El::new("asset-clip")
                .attr("ref", "r2")
                .attr("name", &st)
                .attr("offset", ftime(offset, fps))
                .attr("start", ftime(a, fps))
                .attr("duration", ftime(b - a, fps))
                .attr("format", "r1")
                .attr("tcFormat", "NDF"),
        );
        offset += b - a;
    }
    let seq = El::new("sequence")
        .attr("format", "r1")
        .attr("duration", ftime(tl_len, fps))
        .attr("tcStart", "0s")
        .attr("tcFormat", "NDF")
        .attr("audioLayout", if info.audio_channels >= 2 { "stereo" } else { "mono" })
        .attr("audioRate", if info.audio_rate >= 48000 { "48k" } else { "44.1k" })
        .add(spine);
    let root = El::new("fcpxml")
        .attr("version", "1.9")
        .add(El::new("resources").add(format).add(asset))
        .add(El::new("library").add(El::new("event").attr("name", "CrispTrim").add(El::new("project").attr("name", name).add(seq))));
    pretty(&root, "fcpxml")
}

// --------------------------------------------------------------------------- Premiere (xmeml)

fn rate(fps: Fps) -> El {
    El::new("rate")
        .add(El::text("timebase", fps.as_f64().round_ties_even() as i64))
        .add(El::text("ntsc", if fps.den == 1001 { "TRUE" } else { "FALSE" }))
}

/// Format attendu par Premiere : file://localhost/C%3a/dossier/fichier.mp4
fn pathurl(p: &Path) -> String {
    format!("file://localhost/{}", quote(as_posix(p).trim_start_matches('/')))
}

pub fn premiere_xml(info: &MediaInfo, frames: &Frames, name: &str) -> String {
    let fps = info.fps;
    let total = info.total_frames();
    let tl_len: i64 = frames.iter().map(|(a, b)| b - a).sum();
    let channels: usize = if info.has_audio { info.audio_channels.clamp(1, 2) as usize } else { 0 };
    let fname = file_name(&info.path);
    let has_v = info.has_video;

    let file_el = |first: bool| -> El {
        let mut f = El::new("file").attr("id", "file-1");
        if !first {
            return f;
        }
        f.push(El::text("name", &fname));
        f.push(El::text("pathurl", pathurl(&info.path)));
        f.push(rate(fps));
        f.push(El::text("duration", total));
        let mut fm = El::new("media");
        if has_v {
            fm.push(El::new("video").add(
                El::new("samplecharacteristics").add(rate(fps)).add(El::text("width", info.width)).add(El::text("height", info.height)),
            ));
        }
        if info.has_audio {
            fm.push(
                El::new("audio")
                    .add(El::new("samplecharacteristics").add(El::text("depth", 16)).add(El::text("samplerate", info.audio_rate)))
                    .add(El::text("channelcount", channels)),
            );
        }
        f.push(fm);
        f
    };

    let mut kinds: Vec<&str> = Vec::new();
    if has_v {
        kinds.push("video");
    }
    kinds.extend(std::iter::repeat("audio").take(channels));
    let clip_id = |k: usize, i: usize| format!("clipitem-{}-{}", k + 1, i + 1);
    let audio_index = |k: usize| k - usize::from(has_v) + 1;

    let mut tracks: Vec<El> = kinds.iter().map(|_| El::new("track")).collect();
    let mut first_file = true;
    let mut tl = 0;
    for (i, &(a, b)) in frames.iter().enumerate() {
        for (k, kind) in kinds.iter().enumerate() {
            let mut item = El::new("clipitem").attr("id", clip_id(k, i));
            item.push(El::text("name", &fname));
            item.push(El::text("enabled", "TRUE"));
            item.push(El::text("duration", total));
            item.push(rate(fps));
            item.push(El::text("start", tl));
            item.push(El::text("end", tl + b - a));
            item.push(El::text("in", a));
            item.push(El::text("out", b));
            item.push(file_el(first_file));
            first_file = false;
            if *kind == "audio" {
                item.push(El::new("sourcetrack").add(El::text("mediatype", "audio")).add(El::text("trackindex", audio_index(k))));
            }
            for (k2, kind2) in kinds.iter().enumerate() {
                let idx = if *kind2 == "video" { k2 + 1 } else { audio_index(k2) };
                item.push(
                    El::new("link")
                        .add(El::text("linkclipref", clip_id(k2, i)))
                        .add(El::text("mediatype", kind2))
                        .add(El::text("trackindex", idx))
                        .add(El::text("clipindex", i + 1)),
                );
            }
            tracks[k].push(item);
        }
        tl += b - a;
    }

    let mut media = El::new("media");
    let mut tracks = tracks.into_iter();
    if has_v {
        let fmt = El::new("samplecharacteristics")
            .add(rate(fps))
            .add(El::text("width", info.width))
            .add(El::text("height", info.height))
            .add(El::text("pixelaspectratio", "square"));
        let mut v = El::new("video").add(El::new("format").add(fmt));
        v.push(tracks.next().unwrap());
        media.push(v);
    }
    if channels > 0 {
        let mut au = El::new("audio").add(El::text("numOutputChannels", 2));
        for t in tracks {
            au.push(t);
        }
        media.push(au);
    }
    let seq = El::new("sequence")
        .attr("id", "sequence-1")
        .add(El::text("name", name))
        .add(El::text("duration", tl_len))
        .add(rate(fps))
        .add(media);
    pretty(&El::new("xmeml").attr("version", "4").add(seq), "xmeml")
}

// --------------------------------------------------------------------------- Vegas Pro (EDL texte)

const VEGAS_COLUMNS: [&str; 28] = [
    "ID", "Track", "StartTime", "Length", "PlayRate", "Locked", "Normalized", "StretchMethod", "Looped",
    "OnRuler", "MediaType", "FileName", "Stream", "StreamStart", "StreamLength", "FadeTimeIn", "FadeTimeOut",
    "SustainGain", "CurveIn", "GainIn", "CurveOut", "GainOut", "Layer", "Color", "CurveInR", "CurveOutR",
    "PlayPitch", "LockPitch",
];

/// EDL texte de Vegas Pro (Fichier > Importer > Fichier texte EDL). Temps en millisecondes.
pub fn vegas_edl(info: &MediaInfo, frames: &Frames) -> Vec<u8> {
    let fps = info.fps;
    let ms = |f: i64| (f as f64 * fps.den as f64 * 1000.0) / fps.num as f64;
    let mut kinds = Vec::new();
    if info.has_video {
        kinds.push("VIDEO");
    }
    if info.has_audio {
        kinds.push("AUDIO");
    }
    let mut lines = vec![VEGAS_COLUMNS.iter().map(|c| format!("\"{c}\"")).collect::<Vec<_>>().join(";")];
    let (mut event_id, mut tl) = (1, 0);
    for &(a, b) in frames {
        let (start, length, src) = (ms(tl), ms(b - a), ms(a));
        for (track, kind) in kinds.iter().enumerate() {
            let row: Vec<String> = vec![
                event_id.to_string(), (track + 1).to_string(), format!("{start:.4}"), format!("{length:.4}"),
                "1.000000".into(), "FALSE".into(), "FALSE".into(), "0".into(), "FALSE".into(), "FALSE".into(),
                kind.to_string(), format!("\"{}\"", info.path.display()), "0".into(), format!("{src:.4}"),
                format!("{length:.4}"), "0.0000".into(), "0.0000".into(), "1.000000".into(), "4".into(),
                "0.000000".into(), "4".into(), "0.000000".into(), "0".into(), "-1".into(), "4".into(), "4".into(),
                "0.000000".into(), "FALSE".into(),
            ];
            lines.push(row.join(";"));
            event_id += 1;
        }
        tl += b - a;
    }
    // Vegas est un logiciel Windows : fins de ligne CRLF, UTF-8 avec BOM pour les chemins accentués.
    let mut out = vec![0xEF, 0xBB, 0xBF];
    out.extend((lines.join("\r\n") + "\r\n").into_bytes());
    out
}

pub fn write_fcpxml(info: &MediaInfo, frames: &Frames, out: &Path, name: &str) -> std::io::Result<()> {
    std::fs::write(out, fcpxml(info, frames, name))
}

pub fn write_premiere_xml(info: &MediaInfo, frames: &Frames, out: &Path, name: &str) -> std::io::Result<()> {
    std::fs::write(out, premiere_xml(info, frames, name))
}

pub fn write_vegas_edl(info: &MediaInfo, frames: &Frames, out: &Path) -> std::io::Result<()> {
    std::fs::write(out, vegas_edl(info, frames))
}

// --------------------------------------------------------------------------- MP4 monté

fn x264() -> Vec<String> {
    ["-c:v", "libx264", "-preset", "medium", "-crf", "18"].map(String::from).to_vec()
}

fn video_encoder() -> Vec<String> {
    let enc = tool("ffmpeg").args(["-hide_banner", "-encoders"]).output().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
    // NVENC peut être compilé dans ffmpeg sans carte NVIDIA : on se replie alors sur libx264.
    if enc.contains("h264_nvenc") {
        return ["-c:v", "h264_nvenc", "-preset", "p5", "-rc", "vbr", "-cq", "19", "-b:v", "0"].map(String::from).to_vec();
    }
    x264()
}

fn ffmpeg_major() -> u32 {
    let out = tool("ffmpeg").arg("-version").output().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
    let first = out.lines().next().unwrap_or("");
    let re = regex::Regex::new(r"version n?(\d+)\.").unwrap();
    // versions git (« N-12345-… ») : récentes
    re.captures(first).and_then(|c| c[1].parse().ok()).unwrap_or(99)
}

/// Rend une vidéo nettoyée (réencodage) via ffmpeg trim/concat.
pub fn render_mp4(info: &MediaInfo, frames: &Frames, out: &Path) -> Result<(), String> {
    let fps = info.fps;
    let (mut parts, mut labels) = (String::new(), String::new());
    for (i, &(a, b)) in frames.iter().enumerate() {
        let (s, e) = (fps.secs(a), fps.secs(b));
        if info.has_video {
            parts.push_str(&format!("[0:v]trim=start={s:.6}:end={e:.6},setpts=PTS-STARTPTS[v{i}];"));
            labels.push_str(&format!("[v{i}]"));
        }
        if info.has_audio {
            parts.push_str(&format!("[0:a]atrim=start={s:.6}:end={e:.6},asetpts=PTS-STARTPTS[a{i}];"));
            labels.push_str(&format!("[a{i}]"));
        }
    }
    let (v, a) = (info.has_video as u8, info.has_audio as u8);
    let outs = format!("{}{}", if v == 1 { "[outv]" } else { "" }, if a == 1 { "[outa]" } else { "" });
    let graph = format!("{parts}{labels}concat=n={}:v={v}:a={a}{outs}", frames.len());

    let script = std::env::temp_dir().join(format!("crisptrim-{}-{}.txt", std::process::id(), stem(out)));
    std::fs::write(&script, graph).map_err(|e| e.to_string())?;
    // ffmpeg ≥ 7 lit le graphe avec « -/filter_complex » ; « -filter_complex_script » a disparu en v8.
    let script_opt = if ffmpeg_major() >= 7 { "-/filter_complex" } else { "-filter_complex_script" };

    let run = |encoder: &[String]| -> bool {
        let mut c = tool("ffmpeg");
        c.args(["-nostdin", "-y", "-v", "error", "-stats", "-i"]).arg(&info.path).arg(script_opt).arg(&script);
        if v == 1 {
            c.args(["-map", "[outv]"]).args(encoder).args(["-pix_fmt", "yuv420p", "-r", &format!("{}/{}", fps.num, fps.den)]);
        }
        if a == 1 {
            c.args(["-map", "[outa]", "-c:a", "aac", "-b:a", "192k"]);
        }
        c.args(["-movflags", "+faststart"]).arg(out).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        c.status().map(|s| s.success()).unwrap_or(false)
    };

    let encoder = if v == 1 { video_encoder() } else { x264() };
    let mut ok = run(&encoder);
    if !ok && encoder != x264() {
        ok = run(&x264()); // encodeur matériel indisponible
    }
    let _ = std::fs::remove_file(&script);
    if ok { Ok(()) } else { Err("ffmpeg n'a pas pu rendre la vidéo montée.".into()) }
}

// --------------------------------------------------------------------------- JSON

#[derive(Serialize)]
struct JsonStats {
    duration: f64,
    kept: f64,
    removed: f64,
    removed_pct: f64,
    segments: usize,
    fillers: usize,
}

#[derive(Serialize)]
struct JsonRegion<'a> {
    start: f64,
    end: f64,
    kind: &'a str,
    text: &'a str,
}

#[derive(Serialize)]
struct JsonOut<'a> {
    source: String,
    fps: String,
    threshold_db: f64,
    stats: JsonStats,
    regions: Vec<JsonRegion<'a>>,
    words: &'a [Word],
}

pub fn write_json(info: &MediaInfo, result: &CutResult, words: Option<&[Word]>, threshold_db: f64, out: &Path) -> Result<(), String> {
    let r3 = |x: f64| (x * 1000.0).round_ties_even() / 1000.0;
    let s = result.stats();
    let data = JsonOut {
        source: info.path.display().to_string(),
        fps: format!("{}/{}", info.fps.num, info.fps.den),
        threshold_db: (threshold_db * 10.0).round_ties_even() / 10.0,
        stats: JsonStats { duration: s.duration, kept: s.kept, removed: s.removed, removed_pct: s.removed_pct, segments: s.segments, fillers: s.fillers },
        regions: result.regions.iter().map(|g| JsonRegion { start: r3(g.start), end: r3(g.end), kind: g.kind.as_str(), text: &g.text }).collect(),
        words: words.unwrap_or(&[]),
    };
    let mut buf = Vec::new();
    let fmt = serde_json::ser::PrettyFormatter::with_indent(b" ");
    let mut ser = serde_json::Serializer::with_formatter(&mut buf, fmt);
    data.serialize(&mut ser).map_err(|e| e.to_string())?;
    super::write_text(out, &String::from_utf8_lossy(&buf)).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn info() -> MediaInfo {
        MediaInfo {
            path: PathBuf::from(r"C:\Rushes\mon rush.mp4"),
            duration: 10.0,
            has_video: true,
            fps: Fps::new(25, 1),
            width: 1920,
            height: 1080,
            has_audio: true,
            audio_channels: 2,
            audio_rate: 48000,
            video_codec: "h264".into(),
            pix_fmt: "yuv420p".into(),
        }
    }

    #[test]
    fn fcpxml_shape() {
        let x = fcpxml(&info(), &[(0, 50), (75, 250)], "mon rush (CrispTrim)");
        assert!(x.contains("<format id=\"r1\" frameDuration=\"1/25s\" width=\"1920\" height=\"1080\" />"));
        assert!(x.contains("src=\"file:///C:/Rushes/mon%20rush.mp4\""));
        assert!(x.contains("offset=\"2s\" start=\"3s\" duration=\"7s\""));
        assert!(x.contains("duration=\"9s\""));
    }

    #[test]
    fn premiere_and_vegas() {
        let x = premiere_xml(&info(), &[(0, 50)], "n");
        assert!(x.contains("<pathurl>file://localhost/C%3A/Rushes/mon%20rush.mp4</pathurl>"));
        assert_eq!(x.matches("<clipitem ").count(), 3);
        let v = String::from_utf8(vegas_edl(&info(), &[(25, 50)])[3..].to_vec()).unwrap();
        assert!(v.contains("1;1;0.0000;1000.0000;1.000000;FALSE;FALSE;0;FALSE;FALSE;VIDEO;\"C:\\Rushes\\mon rush.mp4\";0;1000.0000;"));
    }
}
