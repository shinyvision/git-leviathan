//! Media classification: a cheap extension lookup for routing decisions
//! before any bytes are read, and a magic-byte sniff to confirm (or
//! override) once a header is available.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MediaKind {
    Image,
    Audio,
    Video,
}

impl MediaKind {
    pub fn label(self) -> &'static str {
        match self {
            MediaKind::Image => "image",
            MediaKind::Audio => "audio",
            MediaKind::Video => "video",
        }
    }
}

const IMAGE_EXTENSIONS: &[&str] = &[
    "png", "apng", "jpg", "jpeg", "jpe", "jfif", "pjpeg", "gif", "webp", "bmp", "dib", "ico",
    "cur", "tif", "tiff", "tga", "dds", "hdr", "exr", "pnm", "pbm", "pgm", "ppm", "pam", "qoi",
    "ff", "farbfeld", "svg", "avif", "heic", "heif", "jxl", "jp2", "j2k", "psd", "psb", "xbm",
    "xpm", "pcx", "icns", "sgi", "rgb", "pfm", "cr2", "nef", "arw", "dng", "orf", "raf", "rw2",
];

const AUDIO_EXTENSIONS: &[&str] = &[
    "wav", "wave", "mp3", "ogg", "oga", "opus", "flac", "aac", "m4a", "m4b", "aif", "aiff",
    "aifc", "caf", "wma", "mka", "alac", "ac3", "eac3", "dts", "mp2", "mp1", "amr", "au", "snd",
    "ape", "wv", "spx", "tta", "voc", "mpc", "aa3", "oma", "ra", "gsm", "adts", "w64", "rf64",
];

const VIDEO_EXTENSIONS: &[&str] = &[
    "mp4", "m4v", "mov", "qt", "avi", "mkv", "webm", "wmv", "flv", "f4v", "mpg", "mpeg", "m2v",
    "mpe", "ts", "mts", "m2ts", "3gp", "3g2", "ogv", "vob", "rm", "rmvb", "asf", "divx", "mxf",
    "y4m", "dv", "mjpeg", "mjpg", "h264", "264", "h265", "265", "hevc", "av1", "ivf", "nut",
    "gxf", "mpv",
];

fn extension_of(path: &str) -> Option<String> {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let (_, ext) = name.rsplit_once('.')?;
    if ext.is_empty() {
        return None;
    }
    Some(ext.to_ascii_lowercase())
}

/// Classify by file extension only. `None` means "treat as text/binary".
pub fn media_kind_from_path(path: &str) -> Option<MediaKind> {
    let ext = extension_of(path)?;
    let ext = ext.as_str();
    if IMAGE_EXTENSIONS.contains(&ext) {
        Some(MediaKind::Image)
    } else if AUDIO_EXTENSIONS.contains(&ext) {
        Some(MediaKind::Audio)
    } else if VIDEO_EXTENSIONS.contains(&ext) {
        Some(MediaKind::Video)
    } else {
        None
    }
}

/// Classify by the leading bytes of the content. Conservative: returns `None`
/// for anything it doesn't positively recognize, so text files never get
/// hijacked into the media viewer by accident.
pub fn sniff_media_kind(bytes: &[u8]) -> Option<MediaKind> {
    if bytes.len() < 4 {
        return None;
    }
    let starts = |sig: &[u8]| bytes.len() >= sig.len() && &bytes[..sig.len()] == sig;
    let at = |offset: usize, sig: &[u8]| {
        bytes.len() >= offset + sig.len() && &bytes[offset..offset + sig.len()] == sig
    };

    // --- images -----------------------------------------------------------
    if starts(b"\x89PNG\r\n\x1a\n")
        || starts(b"\xFF\xD8\xFF")
        || starts(b"GIF87a")
        || starts(b"GIF89a")
        || starts(b"BM")
        || starts(b"\x00\x00\x01\x00") // ICO
        || starts(b"\x00\x00\x02\x00") // CUR
        || starts(b"II*\x00")
        || starts(b"MM\x00*")
        || starts(b"II+\x00") // BigTIFF
        || starts(b"MM\x00+")
        || starts(b"qoif")
        || starts(b"farbfeld")
        || starts(b"#?RADIANCE")
        || starts(b"#?RGBE")
        || starts(b"\x76\x2f\x31\x01") // OpenEXR
        || starts(b"DDS ")
        || starts(b"8BPS") // PSD
        || starts(b"\xFF\x0A") // JPEG XL codestream
        || starts(b"\x00\x00\x00\x0CJXL ")
        || starts(b"\x00\x00\x00\x0CjP  ") // JPEG 2000
        || starts(b"\xFF\x4F\xFF\x51")
        || starts(b"icns")
        || (starts(b"P") && bytes[1] >= b'1' && bytes[1] <= b'7' && bytes[2].is_ascii_whitespace())
    {
        return Some(MediaKind::Image);
    }
    if starts(b"RIFF") && at(8, b"WEBP") {
        return Some(MediaKind::Image);
    }
    if looks_like_svg(bytes) {
        return Some(MediaKind::Image);
    }

    // --- ISO base media (MP4/MOV/3GP/HEIF/AVIF) ---------------------------
    if at(4, b"ftyp") && bytes.len() >= 12 {
        let brand = &bytes[8..12];
        return Some(match brand {
            b"avif" | b"avis" | b"heic" | b"heix" | b"hevc" | b"hevx" | b"mif1" | b"msf1"
            | b"heim" | b"heis" | b"avio" => MediaKind::Image,
            b"M4A " | b"M4B " | b"M4P " => MediaKind::Audio,
            _ => MediaKind::Video,
        });
    }

    // --- audio ------------------------------------------------------------
    if starts(b"RIFF") && (at(8, b"WAVE") || at(8, b"RMP3")) {
        return Some(MediaKind::Audio);
    }
    if starts(b"RF64") && at(8, b"WAVE") {
        return Some(MediaKind::Audio);
    }
    if starts(b"riff\x2E\x91\xCF\x11") {
        return Some(MediaKind::Audio); // Wave64
    }
    if starts(b"fLaC")
        || starts(b"ID3")
        || starts(b"FORM") && (at(8, b"AIFF") || at(8, b"AIFC"))
        || starts(b"caff")
        || starts(b"MAC ") // Monkey's Audio
        || starts(b"wvpk") // WavPack
        || starts(b".snd") // Sun AU
        || starts(b"#!AMR")
        || starts(b"MPCK")
        || starts(b"MP+")
        || starts(b"TTA1")
        || starts(b"Creative Voice File")
        || starts(b"\x0B\x77") // AC-3 sync
        || starts(b"\x7F\xFE\x80\x01") // DTS
        || starts(b"\xFF\xFB")
        || starts(b"\xFF\xF3")
        || starts(b"\xFF\xF2") // MPEG audio frame sync (no ID3)
        || starts(b"\xFF\xF1")
        || starts(b"\xFF\xF9") // ADTS AAC
    {
        return Some(MediaKind::Audio);
    }
    if starts(b"OggS") {
        // Ogg can carry Vorbis/Opus/FLAC/Speex (audio) or Theora/Daala/VP8 (video).
        let head = &bytes[..bytes.len().min(512)];
        if find(head, b"theora").is_some()
            || find(head, b"daala").is_some()
            || find(head, b"OVP80").is_some()
            || find(head, b"Dirac").is_some()
        {
            return Some(MediaKind::Video);
        }
        return Some(MediaKind::Audio);
    }
    if starts(b"\x1A\x45\xDF\xA3") {
        // Matroska / WebM. `.mka` is audio-only but shares the container; peek
        // for a video track marker.
        let head = &bytes[..bytes.len().min(64 * 1024)];
        if find(head, b"V_").is_some() {
            return Some(MediaKind::Video);
        }
        if find(head, b"A_").is_some() {
            return Some(MediaKind::Audio);
        }
        return Some(MediaKind::Video);
    }
    if starts(b"\x30\x26\xB2\x75\x8E\x66\xCF\x11") {
        // ASF (WMV/WMA) — assume video, FFmpeg will report audio-only.
        return Some(MediaKind::Video);
    }

    // --- video ------------------------------------------------------------
    if starts(b"RIFF") && at(8, b"AVI ") {
        return Some(MediaKind::Video);
    }
    if starts(b"FLV\x01")
        || starts(b"\x00\x00\x01\xBA") // MPEG-PS
        || starts(b"\x00\x00\x01\xB3") // MPEG-1/2 video ES
        || starts(b"YUV4MPEG2")
        || starts(b"DKIF") // IVF
        || starts(b".RMF")
        || starts(b"\x06\x0E\x2B\x34\x02\x05\x01\x01") // MXF
        || starts(b"\x00\x00\x00\x01\x67") // raw H.264 SPS
        || starts(b"\x00\x00\x00\x01\x40") // raw H.265 VPS
        || (bytes[0] == 0x47 && bytes.len() >= 188 * 2 && bytes[188] == 0x47) // MPEG-TS
    {
        return Some(MediaKind::Video);
    }
    if at(4, b"moov") || at(4, b"mdat") || at(4, b"wide") || at(4, b"skip") {
        return Some(MediaKind::Video);
    }

    None
}

fn looks_like_svg(bytes: &[u8]) -> bool {
    // XML text: skip BOM/whitespace, optional <?xml ...?> and comments, then
    // expect an `<svg` root within the first few KB.
    let head = &bytes[..bytes.len().min(4096)];
    let Ok(text) = std::str::from_utf8(head) else {
        // UTF-8 boundary may split a multibyte char; retry a shorter prefix.
        let mut end = head.len();
        while end > 0 && std::str::from_utf8(&head[..end]).is_err() {
            end -= 1;
        }
        return end > 0 && looks_like_svg_text(&head[..end]);
    };
    looks_like_svg_text(text.as_bytes())
}

fn looks_like_svg_text(text: &[u8]) -> bool {
    let text = std::str::from_utf8(text).unwrap_or("");
    let trimmed = text.trim_start_matches('\u{feff}').trim_start();
    if !trimmed.starts_with('<') {
        return false;
    }
    let lower = trimmed.to_ascii_lowercase();
    let mut rest = lower.as_str();
    loop {
        rest = rest.trim_start();
        if rest.starts_with("<?") {
            match rest.find("?>") {
                Some(end) => rest = &rest[end + 2..],
                None => return false,
            }
        } else if rest.starts_with("<!--") {
            match rest.find("-->") {
                Some(end) => rest = &rest[end + 3..],
                None => return false,
            }
        } else if rest.starts_with("<!doctype") {
            match rest.find('>') {
                Some(end) => rest = &rest[end + 1..],
                None => return false,
            }
        } else {
            break;
        }
    }
    rest.starts_with("<svg")
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extension_routing_is_case_insensitive_and_path_aware() {
        assert_eq!(
            media_kind_from_path("assets/Logo.PNG"),
            Some(MediaKind::Image)
        );
        assert_eq!(
            media_kind_from_path("sfx\\hit.WAV"),
            Some(MediaKind::Audio)
        );
        assert_eq!(
            media_kind_from_path("trailer.final.mkv"),
            Some(MediaKind::Video)
        );
        assert_eq!(media_kind_from_path("src/main.rs"), None);
        assert_eq!(media_kind_from_path("Makefile"), None);
        assert_eq!(media_kind_from_path(".gitignore"), None);
        assert_eq!(media_kind_from_path("dir.png/readme"), None);
    }

    #[test]
    fn sniffing_recognizes_common_signatures() {
        assert_eq!(
            sniff_media_kind(b"\x89PNG\r\n\x1a\n\x00\x00"),
            Some(MediaKind::Image)
        );
        assert_eq!(sniff_media_kind(b"\xFF\xD8\xFF\xE0JFIF"), Some(MediaKind::Image));
        assert_eq!(
            sniff_media_kind(b"RIFF\x00\x00\x00\x00WEBPVP8 "),
            Some(MediaKind::Image)
        );
        assert_eq!(
            sniff_media_kind(b"RIFF\x00\x00\x00\x00WAVEfmt "),
            Some(MediaKind::Audio)
        );
        assert_eq!(
            sniff_media_kind(b"RIFF\x00\x00\x00\x00AVI LIST"),
            Some(MediaKind::Video)
        );
        assert_eq!(sniff_media_kind(b"fLaC\x00\x00\x00\x22"), Some(MediaKind::Audio));
        assert_eq!(sniff_media_kind(b"ID3\x04\x00\x00\x00"), Some(MediaKind::Audio));
        assert_eq!(
            sniff_media_kind(b"\x00\x00\x00\x18ftypisom\x00\x00\x02\x00"),
            Some(MediaKind::Video)
        );
        assert_eq!(
            sniff_media_kind(b"\x00\x00\x00\x18ftypM4A \x00\x00\x02\x00"),
            Some(MediaKind::Audio)
        );
        assert_eq!(
            sniff_media_kind(b"\x00\x00\x00\x18ftypavif\x00\x00\x02\x00"),
            Some(MediaKind::Image)
        );
        assert_eq!(
            sniff_media_kind(b"OggS\x00\x02\x00\x00\x00\x00\x00\x00\x00\x00\x01vorbis"),
            Some(MediaKind::Audio)
        );
        assert_eq!(
            sniff_media_kind(b"OggS\x00\x02\x00\x00\x00\x00\x00\x00\x00\x00\x80theora"),
            Some(MediaKind::Video)
        );
        assert_eq!(
            sniff_media_kind(b"\x1A\x45\xDF\xA3\x00\x00A_OPUS"),
            Some(MediaKind::Audio)
        );
        assert_eq!(
            sniff_media_kind(b"\x1A\x45\xDF\xA3\x00\x00V_VP9"),
            Some(MediaKind::Video)
        );
    }

    #[test]
    fn sniffing_detects_svg_after_prolog_and_comments() {
        let svg = b"\xEF\xBB\xBF<?xml version=\"1.0\"?>\n<!-- hi -->\n<!DOCTYPE svg>\n<svg xmlns=\"http://www.w3.org/2000/svg\"/>";
        assert_eq!(sniff_media_kind(svg), Some(MediaKind::Image));
        assert_eq!(sniff_media_kind(b"<html><body>x</body></html>"), None);
    }

    #[test]
    fn sniffing_is_conservative_for_text() {
        assert_eq!(sniff_media_kind(b"fn main() {}\n"), None);
        assert_eq!(sniff_media_kind(b"P"), None);
        assert_eq!(sniff_media_kind(b"Plain text starting with P"), None);
        assert_eq!(sniff_media_kind(b"P6 4 4 255\n"), Some(MediaKind::Image));
    }
}
