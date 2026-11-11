//! Supporters list for the Start page.
//!
//! Both builds read the `supporters.json` that the Pages workflow generates
//! with the Patreon token held in CI: the web build from its own origin, the
//! desktop build from the published site. No Patreon credential is ever
//! compiled into a shipped binary.

/// Supporters who donated outside Patreon (direct transfer, crypto, one-off
/// gifts, …), maintained by hand here. They are merged with the fetched patrons
/// and ranked together by amount, so the combined Start-page list is ordered by
/// pledge regardless of where the donation came from.
///
/// To add a supporter, add a `("Display name", usd_cents)` line below. The
/// amount is in **USD cents**: a $25 donation is `2500`; convert other
/// currencies to USD before adding them.
const MANUAL_SUPPORTERS: &[(&str, i64)] = &[
    ("Stefano", 10_500), // $105
];

/// Append the hand-maintained [`MANUAL_SUPPORTERS`] to `patrons` and sort the
/// combined list by amount (highest first, then alphabetically). Both the
/// native and web boot paths funnel their fetched list through here, so the
/// manual entries — and the ranking — are identical on every platform, and the
/// list is still sorted (and still shows the manual entries) even when the
/// fetch returns nothing because the app is offline or has no Patreon token.
pub fn merge_manual(mut patrons: Vec<(String, i64)>) -> Vec<(String, i64)> {
    for &(name, cents) in MANUAL_SUPPORTERS {
        let name = name.trim();
        if !name.is_empty() {
            patrons.push((name.to_string(), cents));
        }
    }
    patrons.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    patrons
}

#[cfg(not(target_arch = "wasm32"))]
const SUPPORTERS_URL: &str = "https://www.opencadstudio.com/app/supporters.json";

/// Fetch the published supporters list as `(display name, amount in USD
/// cents)`. `Err` when the site cannot be reached or the file is malformed.
#[cfg(not(target_arch = "wasm32"))]
pub fn fetch_patrons() -> Result<Vec<(String, i64)>, String> {
    let body = crate::network::agent(std::time::Duration::from_secs(15))
        .get(SUPPORTERS_URL)
        .header(
            "User-Agent",
            concat!("OpenCADStudio/", env!("OCS_APP_VERSION")),
        )
        .call()
        .map_err(|e| e.to_string())?
        .body_mut()
        .read_to_string()
        .map_err(|e| e.to_string())?;
    parse_supporters(&body)
}

/// `supporters.json` shape: `[{ "name": .., "cents": <USD cents> }]`.
fn parse_supporters(body: &str) -> Result<Vec<(String, i64)>, String> {
    let json: serde_json::Value = serde_json::from_str(body).map_err(|e| e.to_string())?;
    let arr = json.as_array().ok_or("supporters.json is not an array")?;
    Ok(arr
        .iter()
        .filter_map(|e| {
            let name = e["name"].as_str()?.trim().to_string();
            if name.is_empty() {
                return None;
            }
            Some((name, e["cents"].as_i64().unwrap_or(0)))
        })
        .collect())
}

/// Web build: the browser can't call the Patreon API directly (CORS + the
/// token would be exposed in the bundle), so it fetches a pre-generated
/// `supporters.json` published next to the app on the same origin (produced by
/// CI with the token held server-side). Shape:
/// `[{ "name": .., "cents": <USD cents> }]`.
#[cfg(target_arch = "wasm32")]
pub async fn fetch_patrons_web() -> Result<Vec<(String, i64)>, String> {
    use wasm_bindgen::JsCast;
    use wasm_bindgen_futures::JsFuture;

    let window = web_sys::window().ok_or("no window")?;
    let resp_val = JsFuture::from(window.fetch_with_str("supporters.json"))
        .await
        .map_err(|_| "fetch failed")?;
    let resp: web_sys::Response = resp_val.dyn_into().map_err(|_| "not a Response")?;
    if !resp.ok() {
        return Err(format!("HTTP {}", resp.status()));
    }
    let text = JsFuture::from(resp.text().map_err(|_| "text() unavailable")?)
        .await
        .map_err(|_| "body read failed")?;
    let body = text.as_string().ok_or("body is not a string")?;

    parse_supporters(&body)
}


// Convert days since 1970-01-01 to a Gregorian calendar date.
pub(crate) fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    if month <= 2 {
        year += 1;
    }
    (year, month, day)
}
