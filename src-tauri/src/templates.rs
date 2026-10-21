//! Summary templates: what Whiskr pulls out of a recording depends on what it was.

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Template {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Extra instructions for the summary, incl. which sections to produce.
    pub instructions: String,
    #[serde(default)]
    pub custom: bool,
}

fn t(id: &str, name: &str, description: &str, instructions: &str) -> Template {
    Template { id: id.into(), name: name.into(), description: description.into(), instructions: instructions.into(), custom: false }
}

pub fn builtin() -> Vec<Template> {
    vec![
        t("meeting", "Porada", "Týmová schůzka, status, plánování",
          "A team meeting. Sections: \"Probraná témata\" (one line per topic with the outcome), \"Otevřené otázky\" (unresolved points, who should answer)."),
        t("sales", "Obchodní call", "Klient, poptávka, nabídka",
          "A sales / client call. Sections: \"Potřeby klienta\" (pains, goals, context), \"Námitky a obavy\", \"Rozpočet a termíny\" (only if mentioned), \"Další kroky\" (who, what, when)."),
        t("oneonone", "1:1", "Rozhovor dvou lidí, feedback",
          "A one-on-one conversation. Sections: \"Jak se daří\" (mood, workload, blockers), \"Feedback\" (given and received), \"Dohody\"."),
        t("interview", "Pohovor", "Pracovní pohovor s kandidátem",
          "A job interview. Sections: \"Zkušenosti kandidáta\", \"Silné stránky\", \"Rizika a otazníky\", \"Dotazy kandidáta\". Be factual, no verdict unless the speakers stated one."),
        t("lecture", "Přednáška / škola", "Výklad, kurz, školení",
          "A lecture, lesson or training. Sections: \"Klíčové pojmy\" (term — short explanation), \"Hlavní myšlenky\", \"Příklady\", \"Otázky k zopakování\" (5–8 questions to test understanding). Action items only if the lecturer gave homework."),
        t("brainstorm", "Brainstorming", "Nápady, kreativní sezení",
          "A brainstorming session. Sections: \"Nápady\" (every distinct idea, one line each, with who proposed it if clear), \"Nejsilnější nápady\" (the ones the group liked most and why), \"Co ověřit\"."),
        t("notes", "Hlasová poznámka", "Diktát pro sebe, myšlenky",
          "A personal voice note, usually one speaker. Make the overview a clean, well-structured rewrite of the thoughts. Sections: \"Myšlenky\" (bullet points), \"Úkoly pro mě\" only if mentioned."),
    ]
}

pub fn all(custom: &[Template]) -> Vec<Template> {
    let mut v = builtin();
    v.extend(custom.iter().cloned().map(|mut c| {
        c.custom = true;
        c
    }));
    v
}

pub fn find(id: &str, custom: &[Template]) -> Option<Template> {
    all(custom).into_iter().find(|t| t.id == id)
}

/// Lines for the "pick a template" part of the auto prompt.
pub fn menu(custom: &[Template]) -> String {
    all(custom).iter().map(|t| format!("- \"{}\": {} — {}", t.id, t.name, t.description)).collect::<Vec<_>>().join("\n")
}
