//! Host-independent live overlays. Hosts do I/O; this module owns validation,
//! polling policy, last-good snapshots and orbital propagation.
use serde::Serialize;
use serde_json::Value;
use std::collections::HashSet;

#[cfg(not(target_arch = "wasm32"))]
pub mod native;
mod parse;
#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Quakes,
    Hazards,
    Aircraft,
    Satellites,
}
impl Kind {
    pub const ALL: [Self; 4] = [
        Self::Quakes,
        Self::Hazards,
        Self::Aircraft,
        Self::Satellites,
    ];
    pub fn index(self) -> usize {
        self as usize
    }
    pub fn key(self) -> &'static str {
        match self {
            Self::Quakes => "7",
            Self::Hazards => "8",
            Self::Aircraft => "9",
            Self::Satellites => "t",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Quakes => "Quakes",
            Self::Hazards => "Hazards",
            Self::Aircraft => "Aircraft",
            Self::Satellites => "Satellites",
        }
    }
    pub fn source(self) -> &'static str {
        match self {
            Self::Quakes => "USGS",
            Self::Hazards => "NASA EONET",
            Self::Aircraft => "adsb.lol (ODbL)",
            Self::Satellites => "CelesTrak / SGP4",
        }
    }
    pub fn interval(self) -> f64 {
        match self {
            Self::Quakes => 60.,
            Self::Hazards => 900.,
            Self::Aircraft => 15.,
            Self::Satellites => 7200.,
        }
    }
    pub fn lifetime(self) -> f64 {
        match self {
            Self::Quakes => 86400.,
            Self::Hazards => 86400.,
            Self::Aircraft => 120.,
            Self::Satellites => 86400.,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Marker {
    pub id: String,
    pub label: String,
    pub lon: f64,
    pub lat: f64,
    pub detail: String,
    pub url: String,
    pub observed: f64,
    pub magnitude: f64,
    pub heading: Option<f64>,
    pub trail: Vec<(f64, f64)>,
}

pub struct Orbit {
    elements: sgp4::Elements,
    constants: sgp4::Constants,
}

pub struct Layer {
    pub kind: Kind,
    pub enabled: bool,
    pub markers: Vec<Marker>,
    pub updated: Option<f64>,
    pub error: Option<String>,
    pub rejected: usize,
    next_due: f64,
    pending: Option<u32>,
    failures: u32,
    orbits: Vec<Orbit>,
    pub region: Option<(f64, f64)>,
}
impl Layer {
    fn new(kind: Kind) -> Self {
        Self {
            kind,
            enabled: false,
            markers: vec![],
            updated: None,
            error: None,
            rejected: 0,
            next_due: 0.,
            pending: None,
            failures: 0,
            orbits: vec![],
            region: None,
        }
    }
    pub fn visible(&self, now: f64) -> bool {
        self.enabled
            && self
                .updated
                .is_some_and(|t| now - t <= self.kind.lifetime())
    }
    pub fn state(&self, now: f64) -> &'static str {
        if !self.enabled {
            "OFF"
        } else if self.updated.is_some_and(|t| now - t > self.kind.lifetime()) {
            "EXPIRED"
        } else if self.error.is_some() {
            if self.updated.is_some() {
                "STALE"
            } else {
                "OFFLINE"
            }
        } else if self.updated.is_none() {
            if self.pending.is_some() {
                "LOADING"
            } else {
                "WAITING"
            }
        } else if now - self.updated.unwrap() > self.kind.interval() * 2. {
            "STALE"
        } else if self.rejected > 0 {
            "PARTIAL"
        } else if self.markers.is_empty() {
            "EMPTY"
        } else if self.kind == Kind::Satellites {
            "ESTIMATED"
        } else {
            "LIVE"
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Request {
    pub kind: Kind,
    pub id: u32,
    pub url: String,
}

pub struct Feeds {
    pub layers: [Layer; 4],
    pub inspect: bool,
    pub selected: Option<(Kind, String)>,
    pub now: f64,
    serial: u32,
    propagated: f64,
}
impl Default for Feeds {
    fn default() -> Self {
        Self {
            layers: Kind::ALL.map(Layer::new),
            inspect: false,
            selected: None,
            now: 0.,
            serial: 0,
            propagated: 0.,
        }
    }
}
impl Feeds {
    pub fn command(&mut self, key: &str) -> bool {
        if key == "i" {
            self.inspect = !self.inspect;
            return true;
        }
        if let Some(kind) = Kind::ALL.into_iter().find(|k| k.key() == key) {
            let layer = &mut self.layers[kind.index()];
            layer.enabled = !layer.enabled;
            // Keep next_due across toggles: re-enabling must not bypass quotas.
            if !layer.enabled && self.selected.as_ref().is_some_and(|(k, _)| *k == kind) {
                self.selected = None;
            }
            return true;
        }
        false
    }
    pub fn requests(&mut self, now: f64, center: (f64, f64)) -> Vec<Request> {
        if !now.is_finite() || now < 0. {
            return vec![];
        }
        self.now = now;
        self.propagate();
        let mut result = vec![];
        for layer in &mut self.layers {
            if !layer.enabled || layer.pending.is_some() || now < layer.next_due {
                continue;
            }
            self.serial = self.serial.wrapping_add(1);
            layer.pending = Some(self.serial);
            let url = match layer.kind {
                Kind::Quakes => {
                    "https://earthquake.usgs.gov/earthquakes/feed/v1.0/summary/all_day.geojson"
                        .into()
                }
                Kind::Hazards => {
                    "https://eonet.gsfc.nasa.gov/api/v3/events?status=open&days=30&limit=200".into()
                }
                Kind::Aircraft => {
                    let lon = if center.0.is_finite() {
                        (center.0 + 180.).rem_euclid(360.) - 180.
                    } else {
                        0.
                    };
                    let lat = if center.1.is_finite() {
                        center.1.clamp(-90., 90.)
                    } else {
                        0.
                    };
                    // Round requests into degree cells to share browser proxy caches.
                    layer.region = Some((lon.round(), lat.round()));
                    format!(
                        "https://api.adsb.lol/v2/lat/{:.0}/lon/{:.0}/dist/250",
                        lat, lon
                    )
                }
                Kind::Satellites => {
                    "https://celestrak.org/NORAD/elements/gp.php?GROUP=stations&FORMAT=json".into()
                }
            };
            result.push(Request {
                kind: layer.kind,
                id: self.serial,
                url,
            });
        }
        result
    }
    pub fn complete(&mut self, id: u32, result: Result<&str, String>, now: f64) {
        let Some(layer) = self.layers.iter_mut().find(|l| l.pending == Some(id)) else {
            return;
        };
        layer.pending = None;
        let parsed = result.and_then(|text| parse::snapshot(layer.kind, text, now));
        match parsed {
            Ok((mut markers, orbits, rejected)) => {
                if layer.kind == Kind::Aircraft {
                    let old: std::collections::HashMap<_, _> =
                        layer.markers.iter().map(|m| (m.id.as_str(), m)).collect();
                    for m in &mut markers {
                        if let Some(previous) =
                            old.get(m.id.as_str()).filter(|p| now - p.observed < 120.)
                        {
                            m.trail = previous.trail.clone();
                            m.trail.push((previous.lon, previous.lat));
                            if m.trail.len() > 8 {
                                m.trail.remove(0);
                            }
                        }
                    }
                }
                layer.markers = markers;
                layer.orbits = orbits;
                layer.rejected = rejected;
                layer.updated = Some(now);
                layer.error = None;
                layer.failures = 0;
                layer.next_due = now + layer.kind.interval();
                self.propagated = 0.;
            }
            Err(error) => {
                layer.error = Some(error);
                layer.failures = (layer.failures + 1).min(6);
                layer.next_due =
                    now + (layer.kind.interval() * 2f64.powi(layer.failures as i32 - 1)).min(7200.);
            }
        }
        self.now = now;
        self.propagate();
    }
    fn propagate(&mut self) {
        if self.now - self.propagated < 1. {
            return;
        }
        self.propagated = self.now;
        let layer = &mut self.layers[Kind::Satellites.index()];
        if !layer.visible(self.now) {
            return;
        }
        layer.markers = layer
            .orbits
            .iter()
            .filter_map(|orbit| {
                let epoch = orbit.elements.datetime.and_utc().timestamp() as f64;
                if (self.now - epoch).abs() > 7. * 86400. {
                    return None;
                }
                let (lon, lat, alt) = parse::position(orbit, self.now)?;
                let trail = (-15..=15)
                    .filter_map(|i| {
                        parse::position(orbit, self.now + f64::from(i) * 120.)
                            .map(|(x, y, _)| (x, y))
                    })
                    .collect();
                Some(Marker {
                    id: orbit.elements.norad_id.to_string(),
                    label: orbit
                        .elements
                        .object_name
                        .clone()
                        .unwrap_or_else(|| orbit.elements.norad_id.to_string()),
                    lon,
                    lat,
                    detail: format!(
                        "Estimated SGP4 | altitude {alt:.0} km | epoch {} UTC",
                        orbit.elements.datetime
                    ),
                    url: format!(
                        "https://celestrak.org/satcat/table-satcat.php?CATNR={}",
                        orbit.elements.norad_id
                    ),
                    observed: epoch,
                    magnitude: 0.,
                    heading: None,
                    trail,
                })
            })
            .collect();
        if !layer.orbits.is_empty() && layer.markers.is_empty() {
            layer.error = Some("No usable orbits (expired epoch or propagation failure)".into());
        }
    }
    pub fn select(&mut self, projection: &crate::map::Projection, col: u16, row: u16) {
        let mut best = 10;
        self.selected = None;
        for layer in &self.layers {
            if !layer.visible(self.now) {
                continue;
            }
            for m in &layer.markers {
                if let Some((x, y)) = projection.project_point(m.lon, m.lat) {
                    if x < 0 || y < 0 {
                        continue;
                    }
                    let d =
                        (x / 2 + 1 - i32::from(col)).pow(2) + (y / 4 + 1 - i32::from(row)).pow(2);
                    if d < best {
                        best = d;
                        self.selected = Some((layer.kind, m.id.clone()));
                    }
                }
            }
        }
    }
    pub fn status(&self) -> Value {
        Value::Array(self.layers.iter().map(|l|serde_json::json!({"kind":l.kind,"enabled":l.enabled,"state":l.state(self.now),"count":if l.visible(self.now){l.markers.len()}else{0},"updated":l.updated,"error":l.error,"region":l.region})).collect())
    }
}

fn clean(value: &str) -> String {
    value
        .chars()
        .filter(|c| !c.is_control())
        .take(240)
        .collect()
}
fn unique(markers: &mut Vec<Marker>) {
    let mut seen = HashSet::new();
    markers.retain(|m| seen.insert(m.id.clone()));
}
