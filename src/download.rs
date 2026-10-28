//! The launcher's downloader: the era games of the eraGames wiki
//! (wiki.eragames.rip, its "Games" category), each with the git
//! repositories its page links to, cloned as "Clone a game…" does. Only
//! repositories are offered: links to releases and file hosts are left
//! out, and a link to a branch's archive (GitLab's `/-/archive/<branch>/…`)
//! becomes that branch of the repository. The requests go through the
//! wiki's MediaWiki API (`api.php`); fetching them is the launcher's job.

use crate::json::Json;

pub const WIKI: &str = "https://wiki.eragames.rip";

/// A game page of the wiki.
pub struct WikiGame {
    pub title: String,
    pub repos: Vec<Repo>,
}

/// A repository to clone, from a link on a game's page.
#[derive(Clone, Debug, PartialEq)]
pub struct Repo {
    /// what `git clone` is given
    pub url: String,
    /// host/owner/name, to show
    pub path: String,
    /// "" for the repository's default branch
    pub branch: String,
}

impl Repo {
    /// The folder it is cloned into: the repository's name, with the
    /// branch when there is one (two branches of one game side by side).
    pub fn folder(&self) -> String {
        let name = self.path.rsplit('/').next().unwrap_or("game");
        if self.branch.is_empty() {
            name.to_string()
        } else {
            format!("{}-{}", name, self.branch.replace(['/', '\\', ':'], "-"))
        }
    }
}

/// A game page's infobox and first paragraph.
#[derive(Debug, Default, PartialEq)]
pub struct Details {
    /// "Genre", "Translation Status"… as the page has them
    pub facts: Vec<(String, String)>,
    pub about: String,
}

/// `s` %-encoded for a URL (its query, or a user name in it).
pub fn enc(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{:02X}", b) }).collect()
}

/// The request for the Games category's pages and their external links;
/// `more` is the `continue` part of the previous answer.
pub fn list_url(more: &str) -> String {
    format!(
        "{}/api.php?action=query&format=json&formatversion=2&generator=categorymembers&gcmtitle=Category:Games&gcmtype=page&gcmlimit=500&prop=extlinks&ellimit=max{}",
        WIKI, more
    )
}

/// Add an answer to `list_url` to `games` (pages seen before get the new
/// links). Returns the `continue` part for the next request, "" when this
/// was the last; Err when the answer isn't the API's.
pub fn add_list(json: &str, games: &mut Vec<WikiGame>) -> Result<String, String> {
    let j = Json::parse(json).ok_or("the wiki's answer isn't JSON")?;
    if let Some(e) = j.get("error") {
        return Err(format!("the wiki says: {}", e.get("info").and_then(Json::str).unwrap_or("error")));
    }
    for p in j.get("query").and_then(|q| q.get("pages")).map(Json::items).unwrap_or_default() {
        let Some(title) = p.get("title").and_then(Json::str) else { continue };
        let at = match games.iter().position(|g| g.title == title) {
            Some(i) => i,
            None => {
                games.push(WikiGame { title: title.to_string(), repos: Vec::new() });
                games.len() - 1
            }
        };
        for l in p.get("extlinks").map(Json::items).unwrap_or_default() {
            if let Some(r) = l.get("url").and_then(Json::str).and_then(repo_of) {
                let g = &mut games[at];
                if !g.repos.iter().any(|o| o.path.eq_ignore_ascii_case(&r.path) && o.branch == r.branch) {
                    g.repos.push(r);
                }
            }
        }
    }
    Ok(j.get("continue").map_or(String::new(), |c| c.members().iter().map(|(k, v)| format!("&{}={}", enc(k), enc(&value_text(v)))).collect()))
}

fn value_text(v: &Json) -> String {
    match v {
        Json::Str(s) => s.clone(),
        Json::Num(n) => format!("{}", n),
        _ => String::new(),
    }
}

/// The request for a page's first section (its infobox and introduction).
pub fn page_url(title: &str) -> String {
    format!("{}/api.php?action=parse&format=json&formatversion=2&prop=wikitext&section=0&page={}", WIKI, enc(title))
}

/// A page's address, to read it in a browser.
pub fn page_link(title: &str) -> String {
    format!("{}/index.php/{}", WIKI, title.replace(' ', "_"))
}

/// The repository a link on a game's page points to: a gitgud.io, GitLab
/// or GitHub project, a branch of it (`/tree/<branch>`), or a
/// branch's archive. None for anything else (releases, commits, files,
/// user and group pages, other sites).
pub fn repo_of(link: &str) -> Option<Repo> {
    let rest = link.strip_prefix("https://").or_else(|| link.strip_prefix("http://"))?;
    let rest = rest.split(['?', '#']).next()?.trim_end_matches('/');
    let (host, path) = rest.split_once('/')?;
    let host = host.to_ascii_lowercase();
    let host = host.strip_prefix("www.").unwrap_or(&host).to_string();
    let (project, tail) = match host.as_str() {
        "gitgud.io" | "gitlab.com" => match path.split_once("/-/") {
            Some((p, t)) => (p.to_string(), t),
            None => (path.to_string(), ""),
        },
        "github.com" => {
            let mut it = path.splitn(3, '/');
            let (owner, name) = (it.next()?, it.next()?);
            (format!("{}/{}", owner, name), it.next().unwrap_or(""))
        }
        _ => return None,
    };
    let project = project.strip_suffix(".git").unwrap_or(&project).to_string();
    if project.split('/').count() < 2 || project.starts_with("groups/") || project.split('/').any(str::is_empty) {
        return None;
    }
    let seg: Vec<&str> = tail.split('/').filter(|s| !s.is_empty()).collect();
    let branch = match seg.as_slice() {
        [] => String::new(),
        ["tree", b @ ..] if !b.is_empty() => b.join("/"),
        // GitHub: archive/refs/heads/<branch>.zip or archive/<branch>.zip
        ["archive", "refs", "heads", b @ ..] | ["archive", b @ ..] if host == "github.com" && !b.is_empty() => {
            let b = b.join("/");
            [".zip", ".tar.gz"].iter().find_map(|s| b.strip_suffix(s)).unwrap_or(&b).to_string()
        }
        // GitLab: archive/<branch>/<name>.zip
        ["archive", b @ .., _file] if !b.is_empty() => b.join("/"),
        _ => return None,
    };
    Some(Repo { url: format!("https://{}/{}.git", host, project), path: format!("{}/{}", host, project), branch })
}

/// The infobox facts and first paragraph from an answer to `page_url`.
pub fn details(json: &str) -> Result<Details, String> {
    let j = Json::parse(json).ok_or("the wiki's answer isn't JSON")?;
    if let Some(e) = j.get("error") {
        return Err(format!("the wiki says: {}", e.get("info").and_then(Json::str).unwrap_or("error")));
    }
    let text = j.get("parse").and_then(|p| p.get("wikitext")).and_then(Json::str).ok_or("no page text in the wiki's answer")?;
    Ok(read_page(text))
}

/// Read a page's first section: in its tables (the infobox, notices),
/// `| Key: value` rows become facts (a following line that isn't a new
/// row continues the value); the first paragraph outside them is `about`.
pub fn read_page(wikitext: &str) -> Details {
    let mut d = Details::default();
    let (mut tables, mut templates) = (0usize, 0isize);
    let mut cont = false;
    let mut para: Vec<String> = Vec::new();
    for raw in wikitext.lines() {
        let l = raw.trim();
        if l.starts_with("{|") {
            tables += 1;
            cont = false;
            continue;
        }
        if tables > 0 {
            if l.starts_with("|}") {
                tables -= 1;
                cont = false;
            } else if l.starts_with("|-") || l.starts_with("|+") {
                cont = false;
            } else if let Some(row) = l.strip_prefix('|') {
                let row = clean(row);
                cont = false;
                if let Some((k, v)) = row.split_once(':') {
                    let (k, v) = (k.trim(), v.trim());
                    if !k.is_empty() && k.chars().count() <= 30 && !v.is_empty() {
                        d.facts.push((k.to_string(), v.to_string()));
                        cont = true;
                    }
                }
            } else if cont && !l.is_empty() && !l.starts_with('!') {
                let more = clean(l);
                if let Some(f) = d.facts.last_mut() {
                    f.1 = format!("{} {}", f.1, more).trim().to_string();
                }
            } else {
                cont = false;
            }
            continue;
        }
        // templates spanning lines ({{TW}}, navigation boxes)
        let depth = templates + l.matches("{{").count() as isize - l.matches("}}").count() as isize;
        let inside = templates > 0 || depth > 0;
        templates = depth.max(0);
        if inside {
            continue;
        }
        if l.is_empty() || l.starts_with('=') || l.starts_with("__") {
            if !para.is_empty() {
                break;
            }
            continue;
        }
        let c = clean(l);
        if !c.is_empty() {
            para.push(c);
        }
    }
    d.about = para.join(" ");
    d
}

/// Wikitext as plain text: links as their text, no templates, tags, files,
/// categories or bold/italic quotes, spaces collapsed. Emoji variation
/// selectors go too: "⚠️" would take two cells where widths count one.
pub fn clean(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(c) = rest.chars().next() {
        if let Some(r) = rest.strip_prefix("[[") {
            let end = r.find("]]").unwrap_or(r.len());
            let inner = &r[..end];
            let lower = inner.to_ascii_lowercase();
            if !(lower.starts_with("file:") || lower.starts_with("image:") || lower.starts_with("category:")) {
                out.push_str(inner.rsplit('|').next().unwrap_or(inner));
            }
            rest = r.get(end + 2..).unwrap_or("");
        } else if let Some(r) = rest.strip_prefix("{{") {
            rest = r.find("}}").map_or("", |e| &r[e + 2..]);
        } else if let Some(r) = rest.strip_prefix('[').filter(|r| r.starts_with("http")) {
            let end = r.find(']').unwrap_or(r.len());
            if let Some((_, text)) = r[..end].split_once(' ') {
                out.push_str(text);
            }
            rest = r.get(end + 1..).unwrap_or("");
        } else if let Some(r) = rest.strip_prefix('<') {
            // a tag; <br> is a space, <ref>…</ref> goes with its content
            let end = r.find('>').map_or(r.len(), |e| e + 1);
            let tag = r[..end].to_ascii_lowercase();
            rest = r.get(end..).unwrap_or("");
            if tag.starts_with("ref") && !tag.ends_with("/>") {
                rest = rest.find("</ref>").map_or("", |e| &rest[e + 6..]);
            } else if tag.starts_with("br") {
                out.push(' ');
            }
        } else if let Some(r) = rest.strip_prefix("''") {
            rest = r.trim_start_matches('\'');
        } else {
            if !matches!(c, '\u{fe0e}' | '\u{fe0f}') {
                out.push(c);
            }
            rest = &rest[c.len_utf8()..];
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repository_links() {
        let r = |l: &str| repo_of(l).map(|r| (r.url, r.branch));
        let gg = |p: &str, b: &str| Some((format!("https://gitgud.io/{}.git", p), b.to_string()));
        assert_eq!(r("https://gitgud.io/Legis1998/anon-tw"), gg("Legis1998/anon-tw", ""));
        assert_eq!(r("https://gitgud.io/era-games/EraMeiQGaiden/"), gg("era-games/EraMeiQGaiden", ""));
        assert_eq!(
            r("https://gitgud.io/era-games/eraAkumaMaid/-/tree/game/eng-translation?ref_type=heads"),
            gg("era-games/eraAkumaMaid", "game/eng-translation")
        );
        assert_eq!(
            r("https://gitgud.io/era-games/eraMegaten/-/archive/game/eng-release/eraMegaten-game-eng-release.zip"),
            gg("era-games/eraMegaten", "game/eng-release")
        );
        assert_eq!(r("https://gitgud.io/era-games/erarl/-/archive/pops-branch/erarl-eng-translation.zip"), gg("era-games/erarl", "pops-branch"));
        assert_eq!(r("https://gitgud.io/era-games-zh/touhou/eraTW"), gg("era-games-zh/touhou/eraTW", ""));
        assert_eq!(r("https://github.com/napo-sawada/era4X/"), Some(("https://github.com/napo-sawada/era4X.git".into(), String::new())));
        assert_eq!(r("https://github.com/a/b/tree/main"), Some(("https://github.com/a/b.git".into(), "main".into())));
        assert_eq!(r("https://github.com/a/b/archive/refs/heads/dev.zip"), Some(("https://github.com/a/b.git".into(), "dev".into())));
        // not repositories (or not a whole one)
        for l in [
            "https://gitgud.io/Pedy/pedy-tw/-/releases",
            "https://gitgud.io/Pedy/pedy-tw/-/commit/b7baac179a4d2bc982818201f0fe9a4537fcbd5a",
            "https://gitgud.io/era-games",
            "https://gitgud.io/groups/era-games/-/activity",
            "https://files.catbox.moe/bmq8hg.7z",
            "https://seesaawiki.jp/eramegaten_another/",
            "https://github.com/a/b/releases",
            "https://wiki.eragames.rip/bugreport.php?game=TW",
        ] {
            assert_eq!(r(l), None, "{}", l);
        }
        let b = repo_of("https://gitgud.io/era-games/eratohoTW/-/tree/game/eng-release").unwrap();
        assert_eq!(b.folder(), "eratohoTW-game-eng-release");
        assert_eq!(repo_of("https://gitgud.io/era-games/eraAS").unwrap().folder(), "eraAS");
    }

    #[test]
    fn game_list() {
        let page1 = r#"{"continue":{"eloffset":2,"continue":"||"},"query":{"pages":[{"pageid":3,"ns":0,"title":"EraToho TheWorld","extlinks":[{"url":"http://book-shelf-end.com/up2/index.html"},{"url":"https://gitgud.io/era-games/eratohoTW"}]},{"pageid":15,"ns":0,"title":"EraTohoPM"}]}}"#;
        let page2 = r#"{"query":{"pages":[{"pageid":3,"title":"EraToho TheWorld","extlinks":[{"url":"https://gitgud.io/era-games/eratohoTW/-/archive/game/eng-release/x.zip"},{"url":"https://gitgud.io/era-games/eratohoTW/"}]},{"pageid":15,"title":"EraTohoPM","extlinks":[{"url":"https://gitgud.io/era-games/eratohoPM"}]}]}}"#;
        let mut games = Vec::new();
        assert_eq!(add_list(page1, &mut games).unwrap(), "&eloffset=2&continue=%7C%7C");
        assert_eq!(add_list(page2, &mut games).unwrap(), "");
        assert_eq!(games.len(), 2);
        assert_eq!(games[0].repos.iter().map(|r| r.branch.as_str()).collect::<Vec<_>>(), ["", "game/eng-release"]);
        assert_eq!(games[1].repos[0].path, "gitgud.io/era-games/eratohoPM");
        assert!(add_list(r#"{"error":{"code":"x","info":"bad request"}}"#, &mut games).unwrap_err().contains("bad request"));
    }

    #[test]
    fn page_details() {
        let text = "{| class=\"wikitable\"\n|⚠️ '''Note''': No longer maintained. ⚠️<br/>\nThe updated version is now '''[[ShinEraTenseiP|EramegatenP]]'''.<br/>\n|}\n\n{| style=\"float:right\"\n! style=\"x\"| [[File:T.jpg|200px]]\neraMegaten\n|-\n| Genre: Dungeon Crawler/Trainer\n|-\n| Translation Status: Active\n|-\n| [https://gitgud.io/era-games/eraMegatenP Repository link]\n|}\n{{TW\n|x}}\nA fork of [[Pedy's TW]] with ''new'' features.<ref>src</ref>\nSecond line.\n\n= Synopsis =\nNot this.";
        let d = read_page(text);
        assert_eq!(
            d.facts,
            [
                ("⚠ Note".to_string(), "No longer maintained. ⚠ The updated version is now EramegatenP.".to_string()),
                ("Genre".to_string(), "Dungeon Crawler/Trainer".to_string()),
                ("Translation Status".to_string(), "Active".to_string()),
            ]
        );
        assert_eq!(d.about, "A fork of Pedy's TW with new features. Second line.");
        assert_eq!(clean("[[Category:Games]] [https://x.y/z] {{a}}b"), "b");
        assert_eq!(page_url("Pedy's TW"), format!("{}/api.php?action=parse&format=json&formatversion=2&prop=wikitext&section=0&page=Pedy%27s%20TW", WIKI));
    }
}
