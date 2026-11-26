// Query planning for session and knowledge search (plan M5b, measured on
// eval/dev). A plain question becomes content terms: lower-cased, split on
// anything that is not a letter or digit, English and Armenian stopwords
// dropped, a light suffix normaliser applied (Armenian case/article/plural
// endings, English -ing/-ed/-es/-s), 3+ character terms matched as
// prefixes. A source must match at least half of the terms, so unrelated
// questions return nothing instead of noise, and sources that match more
// terms rank first. Queries with phrase quotes or "-term" exclusions keep
// the websearch path (sessions::MATCH_QUERY).

const EN_STOP: &[&str] = &[
    "a", "about", "after", "again", "all", "also", "am", "an", "and", "any", "are", "as", "at",
    "be", "been", "before", "being", "both", "but", "by", "can", "could", "did", "do", "does",
    "doing", "done", "each", "for", "from", "had", "has", "have", "he", "her", "here", "his",
    "how", "i", "if", "in", "into", "is", "it", "its", "just", "me", "more", "most", "my", "no",
    "not", "of", "on", "or", "other", "our", "ours", "she", "should", "so", "some", "such", "than",
    "that", "the", "their", "them", "then", "there", "these", "they", "this", "those", "to", "too",
    "us", "very", "was", "we", "were", "what", "when", "where", "which", "while", "who", "whom",
    "why", "will", "with", "would", "yes", "you", "your",
];

const HY_STOP: &[&str] = &[
    "ինչ",
    "ինչու",
    "ինչպես",
    "ով",
    "ովքեր",
    "որ",
    "որն",
    "որը",
    "որոնք",
    "երբ",
    "որտեղ",
    "ուր",
    "քանի",
    "որքան",
    "արդյոք",
    "եմ",
    "ես",
    "է",
    "ենք",
    "եք",
    "են",
    "էր",
    "էին",
    "ու",
    "և",
    "էլ",
    "իսկ",
    "բայց",
    "թե",
    "եթե",
    "որպեսզի",
    "համար",
    "հետ",
    "մեջ",
    "վրա",
    "մասին",
    "այն",
    "այս",
    "այդ",
    "նա",
    "նրանք",
    "մենք",
    "դու",
    "դուք",
    "մեր",
    "իմ",
    "քո",
    "ձեր",
    "նրա",
    "իր",
    "կա",
    "կան",
    "ունեմ",
    "ունես",
    "ունի",
    "ունենք",
    "ունեք",
    "ունեն",
    "ուներ",
    "ունեին",
    "չէ",
    "ոչ",
    "այո",
    "կամ",
    "միայն",
    "շատ",
    "ավելի",
    "ամեն",
    "բոլոր",
];

// One ending is removed (the longest that leaves a stem of 4+ letters, or
// 3+ after the article or a 3+ letter ending):
// noun case, article and plural endings, and common verb endings, so a
// question's inflected form ("ընտրեցինք", "գինը") still reaches the stored
// form ("ընտրել", "գին...") as a prefix.
const HY_ENDINGS: &[&str] = &[
    "եցինք",
    "եցիք",
    "եցին",
    "եցի",
    "եց",
    "ելու",
    "ելով",
    "ել",
    "ած",
    "ենք",
    "եք",
    "են",
    "ները",
    "ներն",
    "ներում",
    "ներից",
    "ներով",
    "ներին",
    "ների",
    "երում",
    "երից",
    "երով",
    "երին",
    "յում",
    "յից",
    "յով",
    "ներ",
    "երի",
    "ում",
    "ից",
    "ով",
    "ին",
    "յի",
    "ի",
    "ը",
    "ն",
];
const EN_ENDINGS: &[&str] = &["ing", "ed", "es", "s"];

fn armenian(word: &str) -> bool {
    word.chars().any(|c| ('\u{0531}'..='\u{0587}').contains(&c))
}

fn normalise(word: &str) -> String {
    let endings = if armenian(word) {
        HY_ENDINGS
    } else if word.is_ascii() && word.len() >= 5 {
        EN_ENDINGS
    } else {
        &[]
    };
    endings
        .iter()
        .filter_map(|ending| {
            // A 3-letter stem is enough after the article or a long ending.
            let min = if *ending == "ը" || ending.chars().count() >= 3 {
                3
            } else {
                4
            };
            word.strip_suffix(ending)
                .filter(|stem| stem.chars().count() >= min)
        })
        .min_by_key(|stem| stem.chars().count())
        .unwrap_or(word)
        .to_owned()
}

// Armenian emphasis, exclamation and question marks sit inside a word over
// the stressed vowel ("որքա՞ն", "Ո՞ր"); they are not word boundaries.
fn strip_marks(text: &str) -> String {
    text.chars()
        .filter(|c| !matches!(c, '\u{055B}' | '\u{055C}' | '\u{055E}'))
        .collect()
}

// A long Armenian stem may still carry a suffix the list does not know
// (or the stored text uses a shorter form); a shorter prefix catches it.
// Six letters at least: five is about a bare root ("աշխատ" covers both
// "աշխատակից" and "աշխատում"), which matched unrelated sources on eval/dev.
fn truncated(term: &str) -> Option<String> {
    let n = term.chars().count();
    (armenian(term) && n >= 7).then(|| term.chars().take((n - 3).max(6)).collect())
}

/// Content terms of a plain question, or None when the text uses websearch
/// syntax (quotes, -exclusion) or has no content words.
pub(crate) fn terms(text: &str) -> Option<Vec<String>> {
    if text.contains('"') || text.split_whitespace().any(|w| w.starts_with('-')) {
        return None;
    }
    let text = strip_marks(text);
    let mut out: Vec<String> = Vec::new();
    for raw in text.split(|c: char| !c.is_alphanumeric()) {
        let word = raw.to_lowercase();
        if word.chars().count() < 2
            || EN_STOP.contains(&word.as_str())
            || HY_STOP.contains(&word.as_str())
        {
            continue;
        }
        let term = normalise(&word);
        if !out.contains(&term) {
            out.push(term);
        }
    }
    (!out.is_empty()).then_some(out)
}

/// The planned query bound as $2: one group per term joined with " | "
/// (with spaces; `coverage` splits on that), each a quoted lexeme with :*
/// from three characters on; a long Armenian term becomes a group
/// "('term':*|'shorter':*)" with no spaces inside.
pub(crate) fn planned(text: &str) -> Option<String> {
    terms(text).map(|terms| {
        terms
            .iter()
            .map(|t| {
                let lexeme = if t.chars().count() >= 3 {
                    format!("'{t}':*")
                } else {
                    format!("'{t}'")
                };
                match truncated(t) {
                    Some(short) => format!("({lexeme}|'{short}':*)"),
                    None => lexeme,
                }
            })
            .collect::<Vec<_>>()
            .join(" | ")
    })
}

/// How many planned terms a tsvector column matches.
pub(crate) fn coverage(column: &str) -> String {
    format!(
        "(SELECT count(*) FROM unnest(string_to_array($2, ' | ')) t WHERE {column} @@ to_tsquery('simple', t))"
    )
}

/// Minimum number of planned terms a source must match. Measured on
/// eval/dev (2026-11-21): requiring half of the terms gave recall@10 0.85 and
/// abstention 6/6 but dropped answers that match one distinctive word of
/// three or four; one term (stopwords already removed) is the current choice.
pub(crate) const NEED: &str = "1";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_questions_become_content_terms() {
        assert_eq!(
            terms("Who should we target first, centers or individual tutors?").unwrap(),
            ["target", "first", "center", "individual", "tutor"]
        );
        assert_eq!(
            planned("PayLink-ի միջնորդավճար").unwrap(),
            "'paylink':* | ('միջնորդավճար':*|'միջնորդավ':*)"
        );
        assert_eq!(
            terms("Did we choose Stripe?").unwrap(),
            ["choose", "stripe"]
        );
    }

    #[test]
    fn armenian_endings_are_removed_once_and_keep_a_stem() {
        assert_eq!(terms("Ինտերֆեյսի լեզուն").unwrap(), ["ինտերֆեյս", "լեզու"]);
        assert_eq!(terms("Բետայի ամսաթիվը").unwrap(), ["բետա", "ամսաթիվ"]);
        assert_eq!(terms("Ապրիլին Երևանում").unwrap(), ["ապրիլ", "երևան"]);
        assert_eq!(terms("Ինչ գույնի է լոգոն").unwrap(), ["գույն", "լոգո"]);
        assert_eq!(normalise("հին"), "հին", "too short to strip");
        // Over-stripping is harmless: terms are prefixes.
        assert_eq!(
            terms("Ռուսերեն լեզու կլինի՞").unwrap(),
            ["ռուսեր", "լեզու", "կլին"]
        );
    }

    #[test]
    fn armenian_intra_word_marks_do_not_split_words() {
        assert!(terms("Ո՞ր").is_none(), "Ո՞ր is the stopword որ");
        assert_eq!(terms("Որքա՞ն արժե հացը").unwrap(), ["արժե", "հաց"]);
        assert_eq!(
            terms("Ինչո՛ւ փոխեցինք").unwrap(),
            ["փոխ"],
            "ինչու is a stopword once the mark is gone"
        );
        assert_eq!(terms("գինը").unwrap(), ["գին"]);
        // "to have" is a stopword, as in English; stripped it would be 'ուն':*.
        assert_eq!(terms("Քանի աշխատակից ունենք").unwrap(), ["աշխատակ"]);
    }

    #[test]
    fn armenian_verb_forms_reduce_to_a_shared_stem() {
        assert_eq!(terms("ընտրեցինք").unwrap(), ["ընտր"]);
        assert_eq!(terms("ընտրել").unwrap(), ["ընտր"]);
        assert_eq!(terms("պատվիրեցին").unwrap(), ["պատվիր"]);
        assert_eq!(terms("աշխատակիցները").unwrap(), ["աշխատակից"]);
    }

    #[test]
    fn long_armenian_terms_also_match_a_shorter_prefix() {
        assert_eq!(planned("ամսաթիվը").unwrap(), "('ամսաթիվ':*|'ամսաթի':*)");
        assert_eq!(
            planned("մատակարարների գինը").unwrap(),
            "('մատակարար':*|'մատակա':*) | 'գին':*"
        );
        assert_eq!(planned("օգոստոս").unwrap(), "('օգոստոս':*|'օգոստո':*)");
        assert_eq!(
            planned("deliveries schedule").unwrap(),
            "'deliveri':* | 'schedule':*",
            "English is unchanged"
        );
    }

    #[test]
    fn websearch_syntax_and_stopword_only_text_keep_the_old_path() {
        assert!(terms("\"seat founders\"").is_none());
        assert!(terms("pricing -seat").is_none());
        assert!(terms("what is it?").is_none());
        assert!(terms("Ինչ է").is_none());
    }
}
