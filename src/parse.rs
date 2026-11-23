// Türkçe hızlı giriş ayrıştırıcı: "FLIR testi cuma 14:00 ! #Stim"
use chrono::{Datelike, Duration, NaiveDate, NaiveTime, Weekday};

#[derive(Debug, Default, PartialEq)]
pub struct Parsed {
    pub title: String,
    pub date: Option<NaiveDate>,
    pub time: Option<NaiveTime>,
    pub high: bool,
    pub project: Option<String>,
}

pub fn tr_lower(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'İ' => 'i',
            'I' => 'ı',
            _ => c.to_lowercase().next().unwrap_or(c),
        })
        .collect()
}

/// Karşılaştırma için: küçük harf + Türkçe harfleri sadeleştir (aralık = aralik)
pub fn fold(s: &str) -> String {
    tr_lower(s).chars().map(|c| match c {
        'ı' => 'i', 'ş' => 's', 'ğ' => 'g', 'ü' => 'u', 'ö' => 'o', 'ç' => 'c', 'â' => 'a', 'î' => 'i', 'û' => 'u',
        _ => c,
    }).filter(|c| *c != '\u{307}').collect()
}

fn weekday(w: &str) -> Option<Weekday> {
    Some(match w {
        "pazartesi" | "pzt" | "pts" => Weekday::Mon,
        "sali" | "sal" => Weekday::Tue,
        "carsamba" | "car" | "crs" => Weekday::Wed,
        "persembe" | "per" | "prs" => Weekday::Thu,
        "cuma" | "cum" => Weekday::Fri,
        "cumartesi" | "cmt" => Weekday::Sat,
        "pazar" | "paz" => Weekday::Sun,
        _ => return None,
    })
}

fn month(w: &str) -> Option<u32> {
    const M: [(&str, &str); 12] = [
        ("ocak", "oca"), ("subat", "sub"), ("mart", "mar"), ("nisan", "nis"),
        ("mayis", "may"), ("haziran", "haz"), ("temmuz", "tem"), ("agustos", "agu"),
        ("eylul", "eyl"), ("ekim", "eki"), ("kasim", "kas"), ("aralik", "ara"),
    ];
    M.iter().position(|(a, b)| w == *a || w == *b).map(|i| i as u32 + 1)
}

fn parse_time(w: &str) -> Option<NaiveTime> {
    let (h, m) = w.split_once(':')?;
    if h.is_empty() || h.len() > 2 || m.len() != 2 { return None; }
    NaiveTime::from_hms_opt(h.parse().ok()?, m.parse().ok()?, 0)
}

fn future_year(today: NaiveDate, m: u32, d: u32) -> Option<NaiveDate> {
    let x = NaiveDate::from_ymd_opt(today.year(), m, d)?;
    if x < today { NaiveDate::from_ymd_opt(today.year() + 1, m, d) } else { Some(x) }
}

// "25.11" / "25.11.2026" / "25/11"
fn parse_numeric_date(w: &str, today: NaiveDate) -> Option<NaiveDate> {
    let parts: Vec<&str> = w.split(|c| c == '.' || c == '/').collect();
    if parts.len() < 2 || parts.len() > 3 || parts.iter().any(|p| p.is_empty() || !p.chars().all(|c| c.is_ascii_digit())) {
        return None;
    }
    let d: u32 = parts[0].parse().ok()?;
    let m: u32 = parts[1].parse().ok()?;
    if parts.len() == 3 {
        let mut y: i32 = parts[2].parse().ok()?;
        if y < 100 { y += 2000; }
        NaiveDate::from_ymd_opt(y, m, d)
    } else {
        future_year(today, m, d)
    }
}

/// Sadece tarih/saat ifadesi (detay düzenleyicideki deadline alanı için).
/// Boş metin → Some((None, None)) yani deadline kaldır. Anlaşılmazsa None.
pub fn parse_due_only(s: &str, today: NaiveDate, now: NaiveTime) -> Option<(Option<NaiveDate>, Option<NaiveTime>)> {
    if s.trim().is_empty() { return Some((None, None)); }
    let p = parse(s, today, now);
    if !p.title.trim().is_empty() || p.high || p.project.is_some() { return None; }
    if p.date.is_none() && p.time.is_none() { return None; }
    Some((p.date, p.time))
}

pub fn parse(input: &str, today: NaiveDate, now: NaiveTime) -> Parsed {
    let words: Vec<&str> = input.split_whitespace().collect();
    let low: Vec<String> = words.iter().map(|w| fold(w)).collect();
    let mut used = vec![false; words.len()];
    let mut out = Parsed::default();
    let mut i = 0;
    while i < words.len() {
        let w = low[i].as_str();
        let next = low.get(i + 1).map(|s| s.as_str());
        let next2 = low.get(i + 2).map(|s| s.as_str());
        let mut take = 0usize;

        if w.starts_with('#') && w.len() > 1 {
            out.project = Some(words[i][1..].to_string());
            take = 1;
        } else if !w.is_empty() && w.chars().all(|c| c == '!') {
            out.high = true;
            take = 1;
        } else if out.date.is_none() && w == "bugun" {
            out.date = Some(today); take = 1;
        } else if out.date.is_none() && w == "yarin" {
            out.date = Some(today + Duration::days(1)); take = 1;
        } else if out.date.is_none() && w == "oburgun" {
            out.date = Some(today + Duration::days(2)); take = 1;
        } else if out.date.is_none() && w == "obur" && next == Some("gun") {
            out.date = Some(today + Duration::days(2)); take = 2;
        } else if out.date.is_none() && w == "haftaya" {
            out.date = Some(today + Duration::days(7)); take = 1;
        } else if out.date.is_none() && weekday(w).is_some() {
            let wd = weekday(w).unwrap();
            let mut ahead = (wd.num_days_from_monday() as i64 - today.weekday().num_days_from_monday() as i64 + 7) % 7;
            if ahead == 0 { ahead = 7; }
            out.date = Some(today + Duration::days(ahead)); take = 1;
        } else if out.date.is_none() && w.starts_with('+') && w.len() > 1 {
            let n = w[1..].trim_end_matches('g');
            if let Ok(n) = n.parse::<i64>() {
                if n <= 3650 { out.date = Some(today + Duration::days(n)); take = 1; }
            }
        } else if out.time.is_none() && parse_time(w).is_some() {
            out.time = parse_time(w); take = 1;
        } else if out.date.is_none() && parse_numeric_date(w, today).is_some() {
            out.date = parse_numeric_date(w, today); take = 1;
        } else if out.date.is_none() && w.chars().all(|c| c.is_ascii_digit()) && w.len() <= 4 {
            let n: i64 = w.parse().unwrap_or(0);
            if next == Some("gun") && next2 == Some("sonra") && n <= 3650 {
                out.date = Some(today + Duration::days(n)); take = 3;
            } else if (next == Some("gun") || next == Some("gune")) && n <= 3650 && n > 0 {
                out.date = Some(today + Duration::days(n)); take = 2;
            } else if let Some(m) = next.and_then(month) {
                if let Some(d) = future_year(today, m, n as u32) {
                    out.date = Some(d);
                    take = 2;
                    if let Some(y) = next2.and_then(|y| y.parse::<i32>().ok()) {
                        if (2000..2200).contains(&y) {
                            if let Some(d2) = NaiveDate::from_ymd_opt(y, m, n as u32) { out.date = Some(d2); take = 3; }
                        }
                    }
                }
            }
        }

        if take > 0 {
            for k in i..i + take { used[k] = true; }
            i += take;
        } else {
            i += 1;
        }
    }
    if out.time.is_some() && out.date.is_none() {
        let t = out.time.unwrap();
        out.date = Some(if t <= now { today + Duration::days(1) } else { today });
    }
    out.title = words.iter().zip(used.iter()).filter(|(_, u)| !**u).map(|(w, _)| *w).collect::<Vec<_>>().join(" ");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    fn d(y: i32, m: u32, dd: u32) -> NaiveDate { NaiveDate::from_ymd_opt(y, m, dd).unwrap() }
    // 2026-11-23 pazartesi
    fn p(s: &str) -> Parsed { parse(s, d(2026, 11, 23), NaiveTime::from_hms_opt(12, 0, 0).unwrap()) }

    #[test]
    fn basics() {
        let x = p("FLIR testi cuma 14:00 ! #Stim");
        assert_eq!(x.title, "FLIR testi");
        assert_eq!(x.date, Some(d(2026, 11, 27)));
        assert_eq!(x.time, NaiveTime::from_hms_opt(14, 0, 0));
        assert!(x.high);
        assert_eq!(x.project.as_deref(), Some("Stim"));
        assert_eq!(p("Vize çalış 2 Ara").date, Some(d(2026, 12, 2)));
        assert_eq!(p("a 10 ocak").date, Some(d(2027, 1, 10)));
        assert_eq!(p("a 25.11").date, Some(d(2026, 11, 25)));
        assert_eq!(p("a 3 gün sonra").title, "a");
        assert_eq!(p("Lidar 3 aralik 09:30 !").date, Some(d(2026, 12, 3)));
        assert_eq!(p("x Çarşamba").date, Some(d(2026, 11, 25)));
        assert_eq!(p("a +3").date, Some(d(2026, 11, 26)));
        assert_eq!(p("a pazartesi").date, Some(d(2026, 11, 30)));
        assert_eq!(p("a 09:00").date, Some(d(2026, 11, 24)));
        assert_eq!(p("Yarın sabah").date, Some(d(2026, 11, 24)));
        assert_eq!(p("2 motor ekle").title, "2 motor ekle");
        assert_eq!(p("2 motor ekle").date, None);
        assert_eq!(parse_due_only("", d(2026,11,23), NaiveTime::MIN), Some((None, None)));
        assert_eq!(parse_due_only("saçma", d(2026,11,23), NaiveTime::MIN), None);
    }
}
