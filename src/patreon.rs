//! Live Patreon supporters fetch for the Start page.
//!
//! The creator access token is injected at BUILD time via the
//! `OCS_PATREON_TOKEN` environment variable (`option_env!`), so it never lives
//! in the source tree or git history — official release builds set it from a CI
//! secret; other builds simply get an empty list. Note that an embedded token
//! can still be extracted from a shipped binary, so it should be a
//! campaign-scoped token with the minimum needed access.

#[cfg(not(target_arch = "wasm32"))]
const SUPPORT_WINDOW_DAYS: u64 = 31;

#[cfg(not(target_arch = "wasm32"))]
const USD_RATES_URL: &str = "https://api.frankfurter.dev/v2/rates?base=USD";

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
const UA: &str = concat!("OpenCADStudio/", env!("CARGO_PKG_VERSION"));

/// Fetch patrons with a successful payment in the last month from the Patreon
/// API as `(display name, amount in USD cents)`, highest payment first. `Err`
/// when no token is configured or an API call fails.
#[cfg(not(target_arch = "wasm32"))]
pub fn fetch_patrons() -> Result<Vec<(String, i64)>, String> {
    let token = option_env!("OCS_PATREON_TOKEN")
        .filter(|t| !t.is_empty())
        .ok_or("no Patreon token configured")?;

    let agent = crate::network::agent(std::time::Duration::from_secs(15));

    // The token is creator-scoped, so its first campaign is the one to list.
    let campaigns = get_json(&agent, token, "https://www.patreon.com/api/oauth2/v2/campaigns")?;
    let campaign_id = campaigns["data"][0]["id"]
        .as_str()
        .ok_or("no Patreon campaign found for this token")?
        .to_string();
    let usd_rates = fetch_usd_rates(&agent)?;
    let cutoff_date = utc_date_days_ago(SUPPORT_WINDOW_DAYS);

    // Page through the campaign members, keeping only patrons whose latest
    // successful payment falls inside the rolling one-month window.
    let mut patrons: Vec<(String, i64)> = Vec::new();
    let mut url = format!(
        "https://www.patreon.com/api/oauth2/v2/campaigns/{campaign_id}/members\
         ?include=pledge_history\
         &fields%5Bmember%5D=full_name\
         &fields%5Bpledge-event%5D=amount_cents,currency_code,date,payment_status\
         &page%5Bcount%5D=200"
    );
    // Bound the loop so a malformed `next` link can never spin forever.
    for _ in 0..50 {
        let page = get_json(&agent, token, &url)?;
        let included = page["included"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        if let Some(arr) = page["data"].as_array() {
            for m in arr {
                let attrs = &m["attributes"];
                let Some(usd_cents) = latest_recent_paid_usd(
                    m,
                    included,
                    &usd_rates,
                    &cutoff_date,
                ) else {
                    continue;
                };
                let name = attrs["full_name"].as_str().unwrap_or("").trim();
                if !name.is_empty() {
                    patrons.push((name.to_string(), usd_cents));
                }
            }
        }
        match page["links"]["next"].as_str() {
            Some(next) if !next.is_empty() => url = next.to_string(),
            _ => break,
        }
    }

    // Highest pledge first, then alphabetical.
    patrons.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    Ok(patrons)
}

#[cfg(not(target_arch = "wasm32"))]
fn latest_recent_paid_usd(
    member: &serde_json::Value,
    included: &[serde_json::Value],
    usd_rates: &std::collections::HashMap<String, f64>,
    cutoff_date: &str,
) -> Option<i64> {
    let history = member["relationships"]["pledge_history"]["data"].as_array()?;
    let mut latest: Option<(String, i64, String)> = None;

    for relationship in history {
        let Some(id) = relationship["id"].as_str() else {
            continue;
        };
        let Some(resource_type) = relationship["type"].as_str() else {
            continue;
        };
        let Some(event) = included.iter().find(|entry| {
            entry["id"].as_str() == Some(id)
                && entry["type"].as_str() == Some(resource_type)
        }) else {
            continue;
        };
        let attrs = &event["attributes"];
        if attrs["payment_status"].as_str() != Some("Paid") {
            continue;
        }
        let cents = attrs["amount_cents"].as_i64().unwrap_or(0);
        if cents <= 0 {
            continue;
        }
        let date = attrs["date"].as_str().unwrap_or("");
        let Some(event_date) = date.get(..10) else {
            continue;
        };
        if event_date < cutoff_date {
            continue;
        }
        let Some(currency) = attrs["currency_code"].as_str() else {
            continue;
        };
        let currency = currency.trim().to_uppercase();
        if currency.is_empty() {
            continue;
        }
        if latest
            .as_ref()
            .map(|(current, _, _)| date > current.as_str())
            .unwrap_or(true)
        {
            latest = Some((date.to_string(), cents, currency));
        }
    }

    let (_, cents, currency) = latest?;
    let rate = *usd_rates.get(&currency)?;
    if !rate.is_finite() || rate <= 0.0 {
        return None;
    }
    Some((cents as f64 / rate).round() as i64)
}

#[cfg(not(target_arch = "wasm32"))]
fn fetch_usd_rates(
    agent: &ureq::Agent,
) -> Result<std::collections::HashMap<String, f64>, String> {
    let json = get_public_json(agent, USD_RATES_URL)?;
    let entries = json
        .as_array()
        .ok_or("exchange-rate response is not an array")?;
    let mut rates = std::collections::HashMap::new();
    rates.insert("USD".to_string(), 1.0);
    for entry in entries {
        let Some(currency) = entry["quote"].as_str() else {
            continue;
        };
        let Some(rate) = entry["rate"].as_f64() else {
            continue;
        };
        if rate.is_finite() && rate > 0.0 {
            rates.insert(currency.to_uppercase(), rate);
        }
    }
    Ok(rates)
}

#[cfg(not(target_arch = "wasm32"))]
fn utc_date_days_ago(days_ago: u64) -> String {
    let unix_days = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        / 86_400;
    let (year, month, day) = civil_from_days(unix_days as i64 - days_ago as i64);
    format!("{year:04}-{month:02}-{day:02}")
}

// Convert days since 1970-01-01 to a Gregorian calendar date.
#[cfg(not(target_arch = "wasm32"))]
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096)
            / 365;
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

    let json: serde_json::Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
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

#[cfg(not(target_arch = "wasm32"))]
fn get_json(
    agent: &ureq::Agent,
    token: &str,
    url: &str,
) -> Result<serde_json::Value, String> {
    let body = agent
        .get(url)
        .header("Authorization", &format!("Bearer {token}"))
        .header("User-Agent", UA)
        .call()
        .map_err(|e| e.to_string())?
        .body_mut()
        .read_to_string()
        .map_err(|e| e.to_string())?;
    serde_json::from_str(&body).map_err(|e| e.to_string())
}

#[cfg(not(target_arch = "wasm32"))]
fn get_public_json(agent: &ureq::Agent, url: &str) -> Result<serde_json::Value, String> {
    let body = agent
        .get(url)
        .header("User-Agent", UA)
        .call()
        .map_err(|e| e.to_string())?
        .body_mut()
        .read_to_string()
        .map_err(|e| e.to_string())?;
    serde_json::from_str(&body).map_err(|e| e.to_string())
}
