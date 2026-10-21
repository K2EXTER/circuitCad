//! Shopping list: what to buy for the board, and where (shops with a good reach from Japan).
//! A library part can carry the shop's own part number (`Board::part_shops`); every other part gets a link
//! to the shop's search page, so no shop site has to be read by the program.

use crate::model::*;
use std::collections::BTreeMap;

pub struct Shop {
    pub id: &'static str,
    pub name: &'static str,
    /// Search page for a keyword (already percent-encoded).
    search: fn(&str) -> String,
    /// The page of a part number.
    item: fn(&str) -> String,
}

pub const SHOPS: &[Shop] = &[
    Shop {
        id: "akizuki",
        name: "秋月電子通商",
        search: |kw| format!("https://akizukidenshi.com/catalog/goods/search.aspx?search=x&keyword={kw}"),
        item: |code| format!("https://akizukidenshi.com/catalog/g/g{code}/"),
    },
    Shop {
        id: "digikey",
        name: "DigiKey",
        search: |kw| format!("https://www.digikey.jp/ja/products/result?keywords={kw}"),
        item: |code| format!("https://www.digikey.jp/ja/products/result?keywords={}", encode(code)),
    },
    Shop {
        id: "lcsc",
        name: "LCSC (JLCPCB)",
        search: |kw| format!("https://www.lcsc.com/search?q={kw}"),
        item: |code| format!("https://www.lcsc.com/product-detail/{code}.html"),
    },
];

pub fn shop(id: &str) -> Option<&'static Shop> {
    SHOPS.iter().find(|s| s.id == id)
}

/// Percent-encode a query value (UTF-8 bytes; letters, digits and `-._~` stay).
fn encode(text: &str) -> String {
    let mut out = String::new();
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// What to type in a shop's search box: the value and the chip size ("10k R_0805" -> "10k 0805").
pub fn search_keyword(value: &str, footprint: &str) -> String {
    let size = footprint.split(['_', '-']).find(|t| t.len() == 4 && t.chars().all(|c| c.is_ascii_digit())).unwrap_or(footprint);
    format!("{} {}", crate::assembly::bom_value(value), size)
}

/// How a shop is reached for one line of the list.
#[derive(Clone, Debug, PartialEq)]
pub enum Source {
    /// The shop's part number and the page of it.
    Code { number: String, url: String },
    /// Not known: the shop's search page for the value.
    Search { url: String },
}

impl Source {
    pub fn url(&self) -> &str {
        match self {
            Source::Code { url, .. } | Source::Search { url } => url,
        }
    }
    pub fn number(&self) -> &str {
        match self {
            Source::Code { number, .. } => number,
            Source::Search { .. } => "",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Line {
    pub value: String,
    pub footprint: String,
    pub refdes: Vec<String>,
    /// Pieces for one board.
    pub qty: usize,
    /// One entry per shop of `SHOPS`, in the same order.
    pub sources: Vec<Source>,
}

/// One line per distinct (value, footprint, shop numbers).
pub fn lines(board: &Board) -> Vec<Line> {
    type Key = (String, String, BTreeMap<String, String>);
    let mut groups: BTreeMap<Key, Vec<String>> = BTreeMap::new();
    for p in &board.parts {
        let codes = board.part_shops.get(&p.refdes).cloned().unwrap_or_default();
        groups.entry((crate::assembly::bom_value(&p.value), p.footprint.clone(), codes)).or_default().push(p.refdes.clone());
    }
    let mut out: Vec<Line> = groups
        .into_iter()
        .map(|((value, footprint, codes), mut refdes)| {
            refdes.sort_by_key(|r| crate::assembly::natural_key(r));
            let keyword = encode(&search_keyword(&value, &footprint));
            let sources = SHOPS
                .iter()
                .map(|s| match codes.get(s.id).map(|c| c.trim()).filter(|c| !c.is_empty()) {
                    Some(number) => Source::Code { number: number.to_string(), url: (s.item)(number) },
                    None => Source::Search { url: (s.search)(&keyword) },
                })
                .collect();
            Line { qty: refdes.len(), value, footprint, refdes, sources }
        })
        .collect();
    out.sort_by_key(|l| crate::assembly::natural_key(&l.refdes[0]));
    out
}

fn csv(field: &str) -> String {
    if field.contains([',', '"', '\n']) { format!("\"{}\"", field.replace('"', "\"\"")) } else { field.to_string() }
}

/// The list as CSV for `boards` boards: the quantity is the pieces for all of them.
pub fn purchase_csv(board: &Board, boards: usize) -> String {
    let boards = boards.max(1);
    let mut out = String::from("Value,Footprint,Designator,Qty");
    for s in SHOPS {
        out.push_str(&format!(",{0} Part #,{0} Link", s.id));
    }
    out.push('\n');
    for l in lines(board) {
        out.push_str(&format!("{},{},{},{}", csv(&l.value), csv(&l.footprint), csv(&l.refdes.join(" ")), l.qty * boards));
        for s in &l.sources {
            out.push_str(&format!(",{},{}", csv(s.number()), csv(s.url())));
        }
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn board() -> Board {
        let mut b = Board::new("demo", 40.0, 30.0);
        for (r, v, f, x) in [("R1", "10k", "R_0805", 5.0), ("R2", "10k", "R_0805", 10.0), ("U1", "LM358", "SOT-23", 15.0)] {
            b.set_part(Part { refdes: r.into(), value: v.into(), footprint: f.into(), pos: Point::new(x, 10.0), rot: 0, side: Side::Top }).unwrap();
        }
        b
    }

    #[test]
    fn keyword_uses_the_chip_size() {
        assert_eq!(search_keyword("10k", "R_0805"), "10k 0805");
        assert_eq!(search_keyword("100u ESR=0.05", "CP_Elec_6.3x5.4"), "100u CP_Elec_6.3x5.4");
    }

    #[test]
    fn unknown_parts_link_to_a_search_and_known_ones_to_the_page() {
        let mut b = board();
        b.part_shops.insert("U1".into(), BTreeMap::from([("lcsc".to_string(), "C7950".to_string())]));
        let l = lines(&b);
        assert_eq!(l.len(), 2);
        let r = l.iter().find(|x| x.value == "10k").unwrap();
        assert_eq!(r.qty, 2);
        assert!(r.sources.iter().all(|s| matches!(s, Source::Search { .. })));
        assert!(r.sources[2].url().ends_with("q=10k%200805"));
        let u = l.iter().find(|x| x.value == "LM358").unwrap();
        assert_eq!(u.sources[2], Source::Code { number: "C7950".into(), url: "https://www.lcsc.com/product-detail/C7950.html".into() });
        assert!(matches!(u.sources[0], Source::Search { .. }));
    }

    #[test]
    fn same_value_with_other_numbers_stays_apart() {
        let mut b = board();
        b.part_shops.insert("R2".into(), BTreeMap::from([("akizuki".to_string(), "R-00001".to_string())]));
        assert_eq!(lines(&b).iter().filter(|x| x.value == "10k").count(), 2);
    }

    #[test]
    fn csv_multiplies_by_boards() {
        let text = purchase_csv(&board(), 5);
        assert!(text.starts_with("Value,Footprint,Designator,Qty,akizuki Part #,akizuki Link,digikey Part #"));
        assert!(text.contains("10k,R_0805,R1 R2,10,,https://akizukidenshi.com/"));
    }
}
