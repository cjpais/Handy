//! Stop words: prepositions, conjunctions, pronouns, particles, "to be", and
//! the common fillers. They are kept out of the top lists at query time; the
//! database always holds every word, so the switch needs no recount.

use super::analyze::{normalize_form, stem_of};
use std::collections::HashSet;
use std::sync::OnceLock;

const RU: &str = "
я ты он она оно мы вы они меня тебя его ее нас вас их мне тебе ему ей нам вам им мной мною
тобой тобою нами вами ими себя себе собой мой моя мое мои моего моей моих моим моему твой твоя
твое твои наш наша наше наши ваш ваша ваше ваши свой своя свое свои этот эта это эти того тому
том той тем тех та то те такой такая такое такие какой какая какое какие который которая
которое которые кто что чей чья чье чьи где куда откуда когда зачем почему как сколько весь вся
все всё всех всем всеми сам сама само сами каждый любой другой иной один одна одно одни
а но и или да нет не ни ли же бы б ну вот вон там тут здесь туда сюда так также тоже еще ещё
уже только лишь даже очень более менее уж разве ведь мол типа типо короче блин значит просто
вообще прям прямо как-то какой-то что-то кто-то где-то когда-то почему-то потому поэтому затем
потом тогда если чтобы чтоб пока хотя чем нежели либо будто словно
в во на за из изо к ко по о об обо от ото до с со у без безо для при про над под подо перед
передо через между среди из-за из-под около после вместо кроме против возле вокруг
быть был была было были буду будешь будет будем будете будут будучи есть является являются
";

const EN: &str = "
a an the and or but if then so of in on at to for from by with about as into over after before
between through is are was were be been being am do does did have has had will would can could
should may might must i you he she it we they me him her us them my your his its our their this
that these those there here what which who whom whose when where why how not no yes just also
very too than up out down off all any some each other such only own same s t don ll re ve d m
um uh like well
";

pub struct StopSet {
    /// Normalized words of the lists.
    words: HashSet<String>,
    /// Stems of those words: a stem is a stop stem when it is one of these.
    stems: HashSet<String>,
}

impl StopSet {
    fn build() -> Self {
        let mut words = HashSet::new();
        let mut stems = HashSet::new();
        for raw in RU.split_whitespace().chain(EN.split_whitespace()) {
            let form = normalize_form(raw);
            if form.is_empty() {
                continue;
            }
            stems.insert(stem_of(&form));
            words.insert(form);
        }
        Self { words, stems }
    }

    /// A normalized form is a stop word: listed, or an inflection of a listed word.
    pub fn is_stop_form(&self, form: &str) -> bool {
        self.words.contains(form) || self.stems.contains(&stem_of(form))
    }

    pub fn is_stop_stem(&self, stem: &str) -> bool {
        self.stems.contains(stem)
    }

    /// How many stems the list covers (an upper bound of what a query must
    /// over-fetch to still fill a top list after filtering).
    pub fn stem_count(&self) -> usize {
        self.stems.len()
    }
}

pub fn stop_set() -> &'static StopSet {
    static SET: OnceLock<StopSet> = OnceLock::new();
    SET.get_or_init(StopSet::build)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn function_words_in_both_languages_are_stop_words() {
        let set = stop_set();
        for word in ["и", "в", "на", "это", "что", "было", "the", "and", "is"] {
            assert!(set.is_stop_form(word), "{word}");
        }
    }

    #[test]
    fn inflections_of_listed_words_are_stop_words() {
        let set = stop_set();
        assert!(set.is_stop_form("этого"));
        assert!(set.is_stop_form("моими"));
    }

    #[test]
    fn content_words_are_not() {
        let set = stop_set();
        for word in ["промпт", "пайплайн", "обсидиан", "docker", "python", "модель"] {
            assert!(!set.is_stop_form(word), "{word}");
        }
    }

    #[test]
    fn the_list_is_normalized_like_the_text() {
        // ё in the list matches the е of normalized text
        let set = stop_set();
        assert!(set.is_stop_form(&normalize_form("Всё")));
        assert!(set.is_stop_form(&normalize_form("ЕЩЁ")));
    }
}
