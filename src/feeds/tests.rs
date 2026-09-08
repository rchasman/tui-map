use super::*;
const NOW: f64 = 1788800000.;
fn quake(lon: f64) -> String {
    serde_json::json!({"type":"FeatureCollection","features":[{"id":"quake1","geometry":{"type":"Point","coordinates":[lon,20.,12.]},"properties":{"mag":4.5,"place":"Test","time":NOW*1000.,"url":"https://earthquake.usgs.gov/"}}]}).to_string()
}
fn aircraft(lon: f64) -> String {
    serde_json::json!({"ac":[{"hex":"abcdef","flight":"TEST1 ","lon":lon,"lat":20.,"seen_pos":1.,"track":90.}]}).to_string()
}
fn load(feeds: &mut Feeds, key: &str, body: &str, now: f64) {
    feeds.command(key);
    let request = feeds.requests(now, (0., 20.)).pop().unwrap();
    feeds.complete(request.id, Ok(body), now);
}
#[test]
fn disabled_feeds_do_not_fetch_and_toggles_preserve_rate_limits() {
    let mut f = Feeds::default();
    assert!(f.requests(NOW, (0., 0.)).is_empty());
    load(&mut f, "e", &quake(10.), NOW);
    f.command("e");
    f.command("e");
    assert!(f.requests(NOW + 10., (0., 0.)).is_empty());
    assert_eq!(f.requests(NOW + 60., (0., 0.)).len(), 1);
    assert!(f.requests(NOW + 61., (0., 0.)).is_empty());
}
#[test]
fn malformed_refresh_preserves_last_good_snapshot_until_expiry() {
    let mut f = Feeds::default();
    load(&mut f, "e", &quake(10.), NOW);
    let r = f.requests(NOW + 60., (0., 0.)).pop().unwrap();
    f.complete(r.id, Ok(&quake(400.)), NOW + 60.);
    assert_eq!(f.layers[0].state(NOW + 60.), "STALE");
    assert_eq!(f.layers[0].markers[0].lon, 10.);
    assert!(f.layers[0].visible(NOW + 100.));
    assert!(!f.layers[0].visible(NOW + 86401.));
    assert_eq!(f.layers[0].state(NOW + 86401.), "EXPIRED");
}
#[test]
fn loading_is_shown_until_requests_complete() {
    let mut f = Feeds::default();
    f.command("d");
    assert_eq!(f.layers[1].status_label(NOW), "LOADING");
    let request = f.requests(NOW, (0., 0.)).pop().unwrap();
    assert_eq!(f.layers[1].status_label(NOW), "LOADING");
    f.complete(request.id, Ok(r#"{"events":[]}"#), NOW);
    assert_eq!(f.layers[1].status_label(NOW), "0 EMPTY");

    let request = f.requests(NOW + 900., (0., 0.)).pop().unwrap();
    assert_eq!(f.layers[1].status_label(NOW + 900.), "LOADING");
    f.complete(request.id, Err("HTTP 503".into()), NOW + 900.);
    assert_eq!(f.layers[1].state(NOW + 900.), "STALE");
    f.requests(NOW + 1800., (0., 0.));
    assert_eq!(f.layers[1].status_label(NOW + 1800.), "LOADING");
    f.command("d");
    assert_eq!(f.layers[1].state(NOW + 1800.), "OFF");
}
#[test]
fn valid_empty_and_offline_are_distinct() {
    let mut f = Feeds::default();
    load(&mut f, "d", r#"{"events":[]}"#, NOW);
    assert_eq!(f.layers[1].state(NOW), "EMPTY");
    f.command("a");
    let r = f.requests(NOW, (0., 0.)).pop().unwrap();
    f.complete(r.id, Err("HTTP 429".into()), NOW);
    assert_eq!(f.layers[2].state(NOW), "OFFLINE");
    assert!(f.requests(NOW + 14., (0., 0.)).is_empty());
}
#[test]
fn aircraft_requests_are_bounded_and_trails_keep_only_recent_history() {
    let mut f = Feeds::default();
    load(&mut f, "a", &aircraft(1.), NOW);
    let r = f.requests(NOW + 15., (541., -33.9)).pop().unwrap();
    assert_eq!(r.url, "https://api.adsb.lol/v2/lat/-34/lon/-179/dist/250");
    f.complete(r.id, Ok(&aircraft(2.)), NOW + 15.);
    assert_eq!(f.layers[2].markers[0].trail, vec![(1., 20.)]);
    let r = f.requests(NOW + 200., (0., 0.)).pop().unwrap();
    f.complete(r.id, Ok(&aircraft(3.)), NOW + 200.);
    assert!(f.layers[2].markers[0].trail.is_empty());
}
#[test]
fn late_or_duplicate_response_cannot_overwrite_current_snapshot() {
    let mut f = Feeds::default();
    load(&mut f, "e", &quake(10.), NOW);
    f.complete(1, Ok(&quake(20.)), NOW + 1.);
    assert_eq!(f.layers[0].markers[0].lon, 10.);
}
#[test]
fn hazards_choose_newest_geometry_and_sanitize_labels() {
    let body = r#"{"events":[{"id":"event","title":"Storm\u001b\n","categories":[{"title":"Severe Storms"}],"geometry":[{"date":"2026-09-07T00:00:00Z","type":"Point","coordinates":[10,20]},{"date":"2026-09-06T00:00:00Z","type":"Point","coordinates":[30,40]}]}]}"#;
    let (m, _, _) = parse::snapshot(Kind::Hazards, body, NOW).unwrap();
    assert_eq!((m[0].lon, m[0].lat), (10., 20.));
    assert_eq!(m[0].label, "Storm");
}
const OMM: &str = r#"[{"OBJECT_NAME":"ISS (ZARYA)","OBJECT_ID":"1998-067A","EPOCH":"2026-09-07T16:36:29.498976","MEAN_MOTION":15.49021131,"ECCENTRICITY":0.00049803,"INCLINATION":51.6306,"RA_OF_ASC_NODE":251.7517,"ARG_OF_PERICENTER":116.5212,"MEAN_ANOMALY":243.6288,"EPHEMERIS_TYPE":0,"CLASSIFICATION_TYPE":"U","NORAD_CAT_ID":25544,"ELEMENT_SET_NO":999,"REV_AT_EPOCH":58454,"BSTAR":0.00011253134,"MEAN_MOTION_DOT":5.759e-05,"MEAN_MOTION_DDOT":0}]"#;
#[test]
fn omm_propagates_and_expired_elements_are_rejected() {
    let (_, orbits, _) = parse::snapshot(Kind::Satellites, OMM, NOW).unwrap();
    let epoch = orbits[0].elements.datetime.and_utc().timestamp() as f64;
    let a = parse::position(&orbits[0], epoch).unwrap();
    let b = parse::position(&orbits[0], epoch + 600.).unwrap();
    assert!(a.0.abs() <= 180. && a.1.abs() < 52. && (300. ..500.).contains(&a.2));
    assert!((a.0 - b.0).abs() > 1.);
    assert!(parse::snapshot(Kind::Satellites, OMM, epoch + 8. * 86400.).is_err());
    let mut f = Feeds::default();
    load(&mut f, "t", OMM, epoch);
    assert_eq!(f.layers[3].state(epoch), "ESTIMATED");
    assert_eq!(f.layers[3].markers[0].trail.len(), 31);
    let marker = &f.layers[3].markers[0];
    assert_eq!(marker.space_trail.len(), 31);
    assert_eq!(marker.space_trail[15], marker.space_position);
    assert!(marker
        .space_trail
        .iter()
        .flatten()
        .all(|p| (1.04..1.09).contains(&p.length())));
    // A fixed Earth rotation keeps the arc in the orbital plane, instead of
    // twisting it into a future ground track.
    let normal = marker.space_trail[0]
        .unwrap()
        .cross(marker.space_trail[8].unwrap())
        .normalize();
    assert!(marker
        .space_trail
        .iter()
        .flatten()
        .all(|p| normal.dot(*p).abs() < 0.01));
}

#[test]
fn aircraft_retains_geometric_altitude_and_elevated_history() {
    let mut body: Value = serde_json::from_str(&aircraft(1.)).unwrap();
    body["ac"][0]["alt_geom"] = 35000.into();
    body["ac"][0]["alt_baro"] = 34000.into();
    let mut feeds = Feeds::default();
    load(&mut feeds, "a", &body.to_string(), NOW);
    let first = feeds.layers[2].markers[0].space_position.unwrap();
    assert!((feeds.layers[2].markers[0].altitude_km.unwrap() - 10.668).abs() < 1e-9);
    let request = feeds.requests(NOW + 16., (0., 20.)).pop().unwrap();
    body["ac"][0]["lon"] = 2.into();
    body["ac"][0]["alt_geom"] = 36000.into();
    feeds.complete(request.id, Ok(&body.to_string()), NOW + 16.);
    let marker = &feeds.layers[2].markers[0];
    assert_eq!(marker.space_trail, vec![Some(first)]);
    assert!(marker.space_position.unwrap().length() > first.length());
    body["ac"][0]["alt_geom"] = Value::Null;
    body["ac"][0]["alt_baro"] = "ground".into();
    let (markers, _, _) = parse::snapshot(Kind::Aircraft, &body.to_string(), NOW).unwrap();
    assert_eq!(markers[0].altitude_km, Some(0.));
    body["ac"][0]["alt_baro"] = Value::Null;
    let (markers, _, _) = parse::snapshot(Kind::Aircraft, &body.to_string(), NOW).unwrap();
    assert_eq!(markers[0].altitude_km, None);
    assert!(markers[0].space_position.is_none());
}

#[test]
fn hazard_height_is_optional_and_quake_depth_is_not_altitude() {
    let body = r#"{"events":[{"id":"h","title":"Plume","geometry":[{"type":"Point","date":"2026-09-07T00:00:00Z","coordinates":[10,20,6000]}]}]}"#;
    let (markers, _, _) = parse::snapshot(Kind::Hazards, body, NOW).unwrap();
    assert_eq!(markers[0].altitude_km, Some(6.));
    assert!(markers[0].space_position.unwrap().length() > 1.);
    let (markers, _, _) = parse::snapshot(Kind::Quakes, &quake(10.), NOW).unwrap();
    assert!(markers[0].space_position.is_none());
    assert_eq!(markers[0].altitude_km, None);
}

#[test]
fn satellites_beyond_the_limb_are_selectable_at_their_orbital_position() {
    use crate::map::{globe::lonlat_to_vec3, GlobeViewport, Projection};
    let mut feeds = Feeds::default();
    load(&mut feeds, "t", OMM, NOW);
    let globe = Projection::Globe(GlobeViewport::new(0., 0., 80., 200, 200));
    let marker = &mut feeds.layers[3].markers[0];
    marker.lon = 95.;
    marker.lat = 0.;
    marker.space_position = Some(lonlat_to_vec3(95., 0.) * 1.07);
    assert!(globe.project_point(marker.lon, marker.lat).is_none());
    let (x, y) = marker.project(&globe).unwrap();
    feeds.select(&globe, (x / 2 + 1) as u16, (y / 4 + 1) as u16);
    assert!(feeds.selected.is_some());
    feeds.layers[3].markers[0].space_position = Some(lonlat_to_vec3(180., 0.) * 1.07);
    feeds.select(&globe, 51, 26);
    assert!(feeds.selected.is_none());
}
#[test]
fn selection_culls_back_of_globe_and_disabled_layers() {
    use crate::map::{GlobeViewport, Projection};
    let mut f = Feeds::default();
    load(&mut f, "e", &quake(0.), NOW);
    let p = Projection::Globe(GlobeViewport::new(0., 20., 40., 100, 100));
    let (x, y) = p.project_point(0., 20.).unwrap();
    f.select(&p, (x / 2 + 1) as u16, (y / 4 + 1) as u16);
    assert!(f.selected.is_some());
    f.command("e");
    assert!(f.selected.is_none());
}

#[test]
fn aircraft_cache_does_not_refresh_observation_time() {
    let mut value: Value = serde_json::from_str(&aircraft(1.)).unwrap();
    value["now"] = serde_json::json!((NOW - 15.) * 1000.);
    let (markers, _, _) = parse::snapshot(Kind::Aircraft, &value.to_string(), NOW).unwrap();
    assert_eq!(markers[0].observed, NOW - 16.);
}
