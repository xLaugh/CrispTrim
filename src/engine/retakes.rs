//! Reprises : phrase ratée puis recommencée (« Donc la première c'est pour activer le… Donc la
//! première permet de… »), et bégaiements (« on va on va », « le le »). On coupe la tentative ratée
//! et on garde la dernière.
//!
//! Règles réglées sur de vrais rushes :
//! - bégaiement : 1 à 3 mots répétés immédiatement (sauf nombres et « nous nous », « vous vous ») ;
//! - phrase recommencée : au moins 3 mots identiques au début, puis la première tentative bifurque
//!   et s'arrête vite (5 mots de plus au plus, pas de fin de phrase au milieu, moins de 8 s) ;
//! - coordination (« et vous allez copier / et vous allez coller ») : pas une reprise si la
//!   tentative commence ou finit par « et / ou » et dit quelque chose de plus (un mot de contenu).

use super::fillers::{is_filler, normalize};
use super::transcribe::Word;
use std::collections::HashSet;

/// Une reprise à couper : de la première tentative jusqu'au début de la bonne.
#[derive(Clone, Debug, PartialEq)]
pub struct Retake {
    pub start: f64,
    pub end: f64,
    /// Texte de la tentative coupée.
    pub text: String,
}

const MIN_PREFIX: usize = 3;
const MAX_EXTRA: usize = 5;
const MAX_SPAN: f64 = 8.0;
const MAX_STUTTER: usize = 3;
/// Doublons normaux en français, et insistance voulue (« très très », « non non »).
const LEGIT_DOUBLES: [&str; 12] =
    ["nous", "vous", "si", "très", "trop", "vraiment", "beaucoup", "non", "oui", "bien", "encore", "vite"];
/// Petits mots : ne comptent pas comme « mot de contenu ».
const FUNCTION_WORDS: [&str; 44] = [
    "et", "ou", "donc", "puis", "mais", "le", "la", "les", "l", "un", "une", "des", "de", "du", "d", "à", "au", "aux",
    "en", "que", "qu", "qui", "ce", "c", "ça", "on", "je", "j", "tu", "il", "elle", "nous", "vous", "ils", "y", "a",
    "est", "pour", "par", "sur", "dans", "avec", "pas", "ne",
];

fn is_number(w: &str) -> bool {
    const NUMBERS: [&str; 20] = [
        "un", "deux", "trois", "quatre", "cinq", "six", "sept", "huit", "neuf", "dix", "onze", "douze", "treize",
        "quatorze", "quinze", "seize", "vingt", "trente", "cent", "mille",
    ];
    w.chars().all(|c| c.is_ascii_digit()) || NUMBERS.contains(&w)
}

fn ends_sentence(w: &str) -> bool {
    let t = w.trim_end();
    t.ends_with('.') || t.ends_with('?') || t.ends_with('!') || t.ends_with('…')
}

/// Reprises trouvées dans les mots (dans l'ordre). Les hésitations (« euh ») et bruits entre les
/// mots sont ignorés pour comparer, mais coupés avec la tentative.
pub fn find_retakes(words: &[Word]) -> Vec<Retake> {
    let none = HashSet::new();
    // indices des vrais mots (sans hésitations ni bruits entre crochets)
    let idx: Vec<usize> = words
        .iter()
        .enumerate()
        .filter(|(_, w)| {
            let n = normalize(&w.word);
            !n.is_empty() && !n.starts_with('[') && !is_filler(&w.word, &none)
        })
        .map(|(i, _)| i)
        .collect();
    let norm: Vec<String> = idx.iter().map(|&i| normalize(&words[i].word)).collect();
    let content = |k: usize| !FUNCTION_WORDS.contains(&norm[k].as_str()) && !norm[k].is_empty();

    let mut out = Vec::new();
    // répétitions écartées (décalage b - a, jusqu'à quelle position a) : on ne les reprend pas
    // décalées d'un mot plus loin
    let mut blocked: Vec<(usize, usize)> = Vec::new();
    let mut a = 0;
    while a < idx.len() {
        let mut found = None;
        // b : début de la nouvelle tentative, après la position a
        for b in a + 1..idx.len() {
            let span = words[idx[b]].start - words[idx[a]].start;
            if span > MAX_SPAN || b - a > MAX_EXTRA + MAX_STUTTER.max(MIN_PREFIX) + 4 {
                break;
            }
            // longueur du préfixe commun
            let mut k = 0;
            while a + k < b && b + k < idx.len() && norm[a + k] == norm[b + k] {
                k += 1;
            }
            if k == 0 || blocked.iter().any(|&(d, until)| b - a == d && a < until) {
                continue;
            }
            let first_len = b - a; // mots de la première tentative
            // 1) bégaiement : la tentative est entièrement répétée tout de suite
            if k == first_len && k <= MAX_STUTTER {
                let w = &norm[a];
                if k == 1 && (LEGIT_DOUBLES.contains(&w.as_str()) || is_number(w)) {
                    blocked.push((b - a, a + k));
                    break;
                }
                found = Some(b);
                break;
            }
            // 2) phrase recommencée
            if k >= MIN_PREFIX && k < first_len && first_len - k <= MAX_EXTRA {
                let attempt = a..b;
                let pause_mark = |t: usize| {
                    let w = words[idx[t]].word.trim_end();
                    w.ends_with(',') || w.ends_with(';') || w.ends_with(':')
                };
                // La ponctuation montre une phrase menée à son terme, pas abandonnée :
                // - fin de phrase au milieu de la tentative ;
                // - « …votre mot de passe, | mot de passe et là » : la 1re occurrence finissait une phrase ;
                // - « si on veut pas, mais | si on veut… » : virgule au milieu de la partie qui bifurque ;
                // - « j'ai ma clé pour user test, | … pour user Lof » : énumération, un seul mot change.
                let complete = attempt.clone().take(first_len - 1).any(|t| ends_sentence(&words[idx[t]].word))
                    || pause_mark(a + k - 1)
                    || (a + k..b - 1).any(pause_mark)
                    || (pause_mark(b - 1) && first_len - k == 1);
                if complete {
                    blocked.push((b - a, a + k));
                    break;
                }
                // coordination « A et A' » (préfixe court) : garde si la tentative ajoute un mot de contenu
                let coord = |t: usize| matches!(norm[t].as_str(), "et" | "ou" | "puis");
                if k <= 4 && (coord(a) || coord(b - 1)) {
                    let extra_content = (a + k..b).any(|t| content(t) && !coord(t));
                    if extra_content {
                        blocked.push((b - a, a + k));
                        break;
                    }
                }
                found = Some(b);
                break;
            }
        }
        match found {
            Some(b) => {
                let (i0, i1) = (idx[a], idx[b]);
                let prev_end = if i0 > 0 { words[i0 - 1].end } else { 0.0 };
                let start = prev_end.max(words[i0].start - 0.05);
                let end = (words[i1].start - 0.03).max(start);
                let text = words[i0..i1].iter().map(|w| w.word.as_str()).collect::<Vec<_>>().join(" ");
                out.push(Retake { start, end, text });
                a = b;
            }
            None => a += 1,
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Phrase → mots espacés de 0,3 s ; « | » marque une pause d'1 s.
    fn words(s: &str) -> Vec<Word> {
        let mut t = 0.0;
        let mut out = Vec::new();
        for tok in s.split_whitespace() {
            if tok == "|" {
                t += 1.0;
                continue;
            }
            out.push(Word { word: tok.to_string(), start: t, end: t + 0.25 });
            t += 0.3;
        }
        out
    }

    fn cut_texts(s: &str) -> Vec<String> {
        find_retakes(&words(s)).into_iter().map(|r| r.text).collect()
    }

    #[test]
    fn real_restarts_are_cut() {
        assert_eq!(cut_texts("Donc pour ma part, c'est id [UH] tiré du | pour ma part, c'est id underscore ed deux"),
                   vec!["pour ma part, c'est id [UH] tiré du"]);
        assert_eq!(cut_texts("et on peut voir que ma clé privée a été changée, | et on peut voir que ma clé privée s'est bien mise"),
                   vec!["et on peut voir que ma clé privée a été changée,"]);
        assert_eq!(cut_texts("Donc la première c'est pour [UH] activer le Donc la première permet de récupérer"),
                   vec!["Donc la première c'est pour [UH] activer le"]);
        assert_eq!(cut_texts("on a créé et | on a créé un utilisateur dédié."), vec!["on a créé et"]);
        assert_eq!(cut_texts("donc c'est pas vrai donc c'est pas compliqué"), vec!["donc c'est pas vrai"]);
        // long préfixe commun : reprise même avec « et » (le montage garde « … qu'il est allumé »)
        assert_eq!(cut_texts("on peut voir qu'il est en cours d'allumage et | on peut voir qu'il est allumé."),
                   vec!["on peut voir qu'il est en cours d'allumage et"]);
    }

    #[test]
    fn stutters_are_cut() {
        assert_eq!(cut_texts("On va on va ouvrir PowerShell"), vec!["On va"]);
        assert_eq!(cut_texts("mettre le le service en en automatique"), vec!["le", "en"]);
        assert_eq!(cut_texts("on lance l'off- | l'off- ça va nous"), vec!["l'off-"]);
    }

    #[test]
    fn legit_repetitions_are_kept() {
        // coordination : deux actions différentes
        assert!(cut_texts("réouvrir CMD et vous allez copier [UH] et vous allez coller la commande").is_empty());
        assert!(cut_texts("Vous faites CTRL S pour sauvegarder et vous faites CTRL X pour quitter.").is_empty());
        // la ponctuation montre une phrase complète (vrais exemples)
        assert!(cut_texts("il nous demande le mot de passe, donc là vous rentrez votre mot de passe et là on est connecté").is_empty());
        assert!(cut_texts("une passe-phrase si on veut pas, mais si on veut mettre une passe-phrase").is_empty());
        assert!(cut_texts("on va tester avec le root, c'est pareil, et on va tester avec le test").is_empty());
        assert!(cut_texts("j'ai ma clé pour user test, j'ai ma clé pour user Lof").is_empty());
        assert!(cut_texts("utiliser un mot de passe, parce qu'un mot de passe peut être trouvé").is_empty());
        // fin de phrase au milieu : deux phrases distinctes
        assert!(cut_texts("Get Service SSH agent. On peut voir que le service SSH agent a été").is_empty());
        // doublons normaux, nombres, mots isolés éloignés
        assert!(cut_texts("vous vous connectez").is_empty());
        assert!(cut_texts("deux mille vingt-cinq cinq dix-neuf").is_empty());
        // insistance voulue
        assert!(cut_texts("c'est très, très important").is_empty());
        assert!(cut_texts("non non, pas comme ça").is_empty());
    }
}

#[cfg(test)]
mod real_files {
    /// Essai sur de vraies transcriptions : CRISPTRIM_RETAKE_FILES="a.words.json;b.words.json"
    /// cargo test --release retakes_on_files -- --ignored --nocapture
    #[test]
    #[ignore]
    fn retakes_on_files() {
        let files = std::env::var("CRISPTRIM_RETAKE_FILES").unwrap_or_default();
        for f in files.split(';').filter(|f| !f.is_empty()) {
            let words: Vec<super::Word> = serde_json::from_str(&std::fs::read_to_string(f).unwrap()).unwrap();
            let r = super::find_retakes(&words);
            println!("== {f} : {} reprises", r.len());
            for x in r {
                let next: Vec<&str> = words.iter().filter(|w| w.start >= x.end).take(6).map(|w| w.word.as_str()).collect();
                println!("  {:7.1}s  [{}]  >> {}", x.start, x.text, next.join(" "));
            }
        }
    }
}
