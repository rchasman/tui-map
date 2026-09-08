use super::{clean, unique, Kind, Marker, Orbit};
use serde_json::Value;

type Snapshot = (Vec<Marker>, Vec<Orbit>, usize);
pub(super) fn snapshot(kind: Kind, text: &str, now: f64) -> Result<Snapshot, String> {
    if text.len() > 8 * 1024 * 1024 {
        return Err("Feed exceeds 8 MB limit".into());
    }
    let value: Value = serde_json::from_str(text).map_err(|_| "Invalid JSON response")?;
    let rows = match kind {
        Kind::Quakes if value["type"] == "FeatureCollection" => value["features"].as_array(),
        Kind::Hazards => value["events"].as_array(),
        Kind::Aircraft => value["ac"].as_array(),
        Kind::Satellites => value.as_array(),
        _ => None,
    }
    .ok_or("Unexpected feed schema")?;
    if rows.len() > 20000 {
        return Err("Too many feed records".into());
    }
    // ADS-B seen_pos is relative to the provider snapshot, which may have spent
    // time in a proxy cache. Never re-date old positions to the response time.
    let observed_at = if kind == Kind::Aircraft {
        number(&value["now"])
            .map(|t| t / 1000.)
            .filter(|t| *t <= now + 300.)
            .unwrap_or(now)
    } else {
        now
    };
    let mut markers = vec![];
    let mut orbits = vec![];
    let mut rejected = 0;
    for row in rows {
        if kind == Kind::Satellites {
            let orbit = serde_json::from_value::<sgp4::Elements>(row.clone())
                .ok()
                .and_then(|elements| {
                    let age = (now - elements.datetime.and_utc().timestamp() as f64).abs();
                    if age > 7. * 86400. {
                        return None;
                    }
                    let constants = sgp4::Constants::from_elements(&elements).ok()?;
                    let orbit = Orbit {
                        elements,
                        constants,
                    };
                    position(&orbit, now)?;
                    Some(orbit)
                });
            if let Some(orbit) = orbit {
                orbits.push(orbit);
            } else {
                rejected += 1;
            }
        } else if let Some(marker) = marker(kind, row, observed_at) {
            markers.push(marker);
        } else {
            rejected += 1;
        }
    }
    // Positioned aircraft are a subset of a valid receiver snapshot; ground / old
    // messages without coordinates do not make an otherwise valid empty region fail.
    if !rows.is_empty() && markers.is_empty() && orbits.is_empty() && kind != Kind::Aircraft {
        return Err("No valid records in response".into());
    }
    unique(&mut markers);
    markers.truncate(5000);
    Ok((markers, orbits, rejected))
}
fn number(v: &Value) -> Option<f64> {
    v.as_f64().filter(|x| x.is_finite())
}
fn coords(v: &Value) -> Option<(f64, f64)> {
    let lon = number(&v[0])?;
    let lat = number(&v[1])?;
    (lon.abs() <= 180. && lat.abs() <= 90.).then_some((lon, lat))
}
fn date(v: &Value) -> Option<f64> {
    sgp4::chrono::DateTime::parse_from_rfc3339(v.as_str()?)
        .ok()
        .map(|t| t.timestamp() as f64)
}
fn text(v: &Value) -> String {
    clean(v.as_str().unwrap_or(""))
}
fn link(v: &Value) -> String {
    let s = text(v);
    if s.starts_with("https://") {
        s
    } else {
        String::new()
    }
}
fn marker(kind: Kind, v: &Value, now: f64) -> Option<Marker> {
    let (id, label, lon, lat, detail, url, observed, magnitude, heading) = match kind {
        Kind::Quakes => {
            if v["geometry"]["type"] != "Point" {
                return None;
            }
            let (lon, lat) = coords(&v["geometry"]["coordinates"])?;
            let p = &v["properties"];
            let mag = number(&p["mag"])?;
            let observed = number(&p["time"])? / 1000.;
            if now - observed > 86400. || observed > now + 300. {
                return None;
            }
            (
                text(&v["id"]),
                format!("M{mag:.1} {}", text(&p["place"])),
                lon,
                lat,
                format!(
                    "Magnitude {mag:.1} | depth {:.1} km | {}",
                    number(&v["geometry"]["coordinates"][2]).unwrap_or(0.),
                    text(&p["status"])
                ),
                link(&p["url"]),
                observed,
                mag,
                None,
            )
        }
        Kind::Hazards => {
            let geometry = v["geometry"]
                .as_array()?
                .iter()
                .filter_map(|g| date(&g["date"]).map(|d| (d, g)))
                .max_by(|a, b| a.0.total_cmp(&b.0))?;
            if geometry.1["type"] != "Point" {
                return None;
            }
            let (lon, lat) = coords(&geometry.1["coordinates"])?;
            (
                text(&v["id"]),
                text(&v["title"]),
                lon,
                lat,
                text(&v["categories"][0]["title"]),
                link(&v["sources"][0]["url"]),
                geometry.0,
                0.,
                None,
            )
        }
        Kind::Aircraft => {
            let lon = number(&v["lon"])?;
            let lat = number(&v["lat"])?;
            if lon.abs() > 180. || lat.abs() > 90. {
                return None;
            }
            let age = number(&v["seen_pos"])?;
            if !(0. ..=60.).contains(&age) {
                return None;
            }
            let id = text(&v["hex"]);
            let flight = text(&v["flight"]).trim().to_owned();
            let label = if flight.is_empty() {
                id.clone()
            } else {
                flight
            };
            let altitude = number(&v["alt_baro"])
                .map(|x| format!("{x:.0} ft"))
                .unwrap_or_else(|| {
                    if v["alt_baro"] == "ground" {
                        "ground".into()
                    } else {
                        "altitude unknown".into()
                    }
                });
            let speed = number(&v["gs"])
                .map(|x| format!("{x:.0} kt"))
                .unwrap_or_else(|| "speed unknown".into());
            (
                id,
                label,
                lon,
                lat,
                format!(
                    "{} | {} | {altitude} | {speed}",
                    text(&v["r"]),
                    text(&v["t"])
                ),
                "https://www.adsb.lol/".into(),
                now - age,
                0.,
                number(&v["track"]),
            )
        }
        Kind::Satellites => return None,
    };
    if id.is_empty() {
        return None;
    }
    Some(Marker {
        id,
        label,
        lon,
        lat,
        detail,
        url,
        observed,
        magnitude,
        heading,
        trail: vec![],
    })
}

/// TEME -> Earth-fixed longitude and WGS84 geodetic latitude. SGP4 supplies
/// kilometres; sidereal rotation is evaluated at the prediction's UTC time.
pub(super) fn position(orbit: &Orbit, seconds: f64) -> Option<(f64, f64, f64)> {
    let datetime = sgp4::chrono::DateTime::from_timestamp(seconds as i64, 0)?.naive_utc();
    let minutes = orbit
        .elements
        .datetime_to_minutes_since_epoch(&datetime)
        .ok()?;
    let prediction = orbit.constants.propagate(minutes).ok()?;
    let [x, y, z] = prediction.position;
    if ![x, y, z].iter().all(|x| x.is_finite()) {
        return None;
    }
    let theta = sgp4::afspc_epoch_to_sidereal_time(sgp4::julian_years_since_j2000(&datetime));
    let lon = (y.atan2(x) - theta).to_degrees();
    let lon = (lon + 180.).rem_euclid(360.) - 180.;
    let p = x.hypot(y);
    let e2 = 0.00669437999014;
    let mut lat = z.atan2(p);
    for _ in 0..8 {
        let n = 6378.137 / (1. - e2 * lat.sin().powi(2)).sqrt();
        lat = (z + e2 * n * lat.sin()).atan2(p);
    }
    let n = 6378.137 / (1. - e2 * lat.sin().powi(2)).sqrt();
    let altitude = if lat.cos().abs() > 1e-8 {
        p / lat.cos() - n
    } else {
        z.abs() - 6356.752314
    };
    (altitude >= 0.).then_some((lon, lat.to_degrees(), altitude))
}
