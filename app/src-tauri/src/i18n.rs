//! Interface language. English strings are the keys; [`tr`] maps them to
//! French. Anything without a translation falls back to English, so a
//! missing entry is never worse than an untranslated word.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    /// Follow the macOS language.
    #[default]
    System,
    En,
    Fr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Lang {
    En,
    Fr,
}

impl Language {
    pub fn resolve(self) -> Lang {
        match self {
            Language::En => Lang::En,
            Language::Fr => Lang::Fr,
            Language::System => system(),
        }
    }
}

#[cfg(target_os = "macos")]
fn system() -> Lang {
    use objc2_foundation::NSLocale;
    let first = NSLocale::preferredLanguages()
        .firstObject()
        .map(|s| s.to_string())
        .unwrap_or_default();
    if first.starts_with("fr") {
        Lang::Fr
    } else {
        Lang::En
    }
}

#[cfg(not(target_os = "macos"))]
fn system() -> Lang {
    match std::env::var("LANG") {
        Ok(l) if l.starts_with("fr") => Lang::Fr,
        _ => Lang::En,
    }
}

/// Translate an interface string.
pub fn tr(lang: Lang, en: &'static str) -> &'static str {
    if lang == Lang::En {
        return en;
    }
    match en {
        // Tray menu
        "Open Wheel for Finder Selection" => "Ouvrir la roue pour la sélection du Finder",
        "Show Float Shelf" => "Afficher l’étagère Float",
        "Enabled" => "Activé",
        "Settings…" => "Réglages…",
        "Quit Clémentine" => "Quitter Clémentine",
        // Windows, dialogs and notifications
        "Clémentine Settings" => "Réglages de Clémentine",
        "Move to…" => "Déplacer vers…",
        "Move failed" => "Échec du déplacement",
        "Select files in Finder first." => "Sélectionnez d’abord des fichiers dans le Finder.",
        "Done" => "Terminé",
        "finished" => "terminé",
        "failed" => "a échoué",
        "files" => "fichiers",
        "items" => "éléments",
        "This shortcut can't be used" => "Ce raccourci n’est pas utilisable",
        _ => en,
    }
}

/// Wheel label for an operation. Format names (PDF, WEBP…) are the same in
/// every language and pass through.
pub fn op_label(lang: Lang, id: &str, label: &str) -> String {
    if lang == Lang::En {
        return label.to_string();
    }
    let fr = match id {
        "convert:extract" => "Extraire",
        "tool:compress" => "Compresser",
        "tool:resize-50" => "Réduire 50 %",
        "tool:resize-1080" if label.contains("px") => "Max 1080 px",
        "tool:rotate-cw" => "Pivoter ↻",
        "tool:rotate-ccw" => "Pivoter ↺",
        "tool:flip-h" => "Miroir",
        "tool:grayscale" => "Niveaux de gris",
        "tool:strip-metadata" => "Sans métadonnées",
        "tool:ocr" if label == "OCR text" => "Texte (OCR)",
        "tool:ocr" => "Extraire le texte",
        "tool:remove-bg" => "Détourer",
        "tool:merge-pdf" => "Combiner en PDF",
        "tool:merge" if label == "Merge sheets" => "Fusionner",
        "tool:merge" => "Fusionner",
        "tool:extract-audio" => "Extraire l’audio",
        "tool:mute" => "Couper le son",
        "tool:speed-2x" => "Vitesse ×2",
        "tool:thumbnail" => "Miniature",
        "tool:normalize" => "Normaliser",
        "tool:trim-silence" => "Couper les silences",
        "tool:split" => "Séparer les pages",
        "tool:first-page" => "Première page",
        "tool:shift+1" => "Retarder +1 s",
        "tool:shift-1" => "Avancer −1 s",
        "tool:archive" => "Zipper ensemble",
        "action:float" => "Float",
        "action:copy" => "Copier",
        "action:move" => "Déplacer…",
        "action:copy-path" => "Copier le chemin",
        "action:terminal" => "Terminal",
        _ => return label.to_string(),
    };
    fr.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn falls_back_to_english() {
        assert_eq!(tr(Lang::Fr, "Settings…"), "Réglages…");
        assert_eq!(tr(Lang::Fr, "no such string"), "no such string");
        assert_eq!(op_label(Lang::Fr, "convert:webp", "WEBP"), "WEBP");
        assert_eq!(
            op_label(Lang::Fr, "tool:remove-bg", "Remove BG"),
            "Détourer"
        );
        assert_eq!(
            op_label(Lang::En, "tool:remove-bg", "Remove BG"),
            "Remove BG"
        );
    }
}
