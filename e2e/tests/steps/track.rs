use std::time::Duration;

use cucumber::then;
use medea_e2e::object::{MediaKind, MediaSourceKind};
use tokio::time::sleep;

use crate::{steps::parse_media_kinds, world::World};

#[then(regex = r"^(\S+) has (\d+) local track(?:s)?$")]
async fn then_member_has_local_tracks(
    world: &mut World,
    id: String,
    count: u64,
) {
    world.wait_for_interconnection(&id).await.unwrap();
    let member = world.get_member(&id).unwrap();
    let room = member.room();
    let tracks = room.local_tracks().await.unwrap();
    assert_eq!(count, tracks.count().await.unwrap());
}

#[then(regex = "^(\\S+) has (audio|video|audio and video) remote \
                 track(?:s)? from (\\S+)$")]
async fn then_member_has_remote_track(
    world: &mut World,
    id: String,
    kind: String,
    remote_id: String,
) {
    let member = world.get_member(&id).unwrap();
    let connection = member
        .connections()
        .wait_for_connection(remote_id)
        .await
        .unwrap();
    let tracks_store = connection.tracks_store().await.unwrap();

    if kind.contains("audio") {
        tracks_store
            .get_track(MediaKind::Audio, MediaSourceKind::Device)
            .await
            .unwrap();
    }
    if kind.contains("video") {
        tracks_store
            .get_track(MediaKind::Video, MediaSourceKind::Device)
            .await
            .unwrap();
    }
}

#[then(regex = r"^(\S+) has local (audio|(?:device |display )?video)$")]
async fn then_has_local_track(world: &mut World, id: String, kind: String) {
    let member = world.get_member(&id).unwrap();
    let room = member.room();
    let tracks = room.local_tracks().await.unwrap();
    let media_kind = kind.parse().unwrap();

    let mut source_kinds = Vec::with_capacity(2);
    if let Ok(kind) = kind.parse() {
        source_kinds.push(kind);
    } else {
        if media_kind == MediaKind::Video {
            source_kinds.push(MediaSourceKind::Display);
        }
        source_kinds.push(MediaSourceKind::Device);
    }
    for source_kind in source_kinds {
        tracks.get_track(media_kind, source_kind).await.unwrap();
    }
}

#[then(regex = "^(\\S+)'s remote (audio|(?:device|display) video) track \
                 from (\\S+) disables$")]
async fn then_remote_track_stops(
    world: &mut World,
    id: String,
    kind: String,
    remote_id: String,
) {
    let member = world.get_member(&id).unwrap();
    let (media_kind, source_kind) = parse_media_kinds(&kind).unwrap();

    let conn = member.connections().get(remote_id).await.unwrap().unwrap();
    let track = conn
        .tracks_store()
        .await
        .unwrap()
        .get_track(media_kind, source_kind)
        .await
        .unwrap();
    assert!(track.disabled().await.unwrap());
}

#[then(regex = "^`on_(enabled|disabled|muted|unmuted)` callback fires \
                 (\\d+) time(?:s)? on (\\S+)'s \
                 remote (audio|(?:device|display) video) track from (\\S+)$")]
async fn then_callback_fires_on_remote_track(
    world: &mut World,
    callback_kind: String,
    times: u64,
    id: String,
    kind: String,
    remote_id: String,
) {
    let member = world.get_member(&id).unwrap();
    let remote_conn =
        member.connections().get(remote_id).await.unwrap().unwrap();
    let (media_kind, source_kind) = parse_media_kinds(&kind).unwrap();
    let track = remote_conn
        .tracks_store()
        .await
        .unwrap()
        .get_track(media_kind, source_kind)
        .await
        .unwrap();

    match callback_kind.as_str() {
        "enabled" => {
            track.wait_for_on_enabled_fire_count(times).await.unwrap();
        }
        "disabled" => {
            track.wait_for_on_disabled_fire_count(times).await.unwrap();
        }
        "muted" => {
            track.wait_for_on_muted_fire_count(times).await.unwrap();
        }
        "unmuted" => {
            track.wait_for_on_unmuted_fire_count(times).await.unwrap();
        }
        _ => {
            unreachable!(
                "unknown RemoteMediaTrack callback: `on_{callback_kind}`",
            );
        }
    }
}

#[then(regex = "^(\\S+)'s (audio|(?:display|device) video) remote track \
                 from (\\S+) is (enabled|disabled)$")]
async fn then_remote_media_track(
    world: &mut World,
    id: String,
    kind: String,
    partner_id: String,
    state: String,
) {
    let member = world.get_member(&id).unwrap();
    let partner_connection = member
        .connections()
        .wait_for_connection(partner_id)
        .await
        .unwrap();
    let tracks_with_partner = partner_connection.tracks_store().await.unwrap();

    let (media_kind, source_kind) = parse_media_kinds(&kind).unwrap();
    let track = tracks_with_partner
        .get_track(media_kind, source_kind)
        .await
        .unwrap();
    sleep(Duration::from_millis(500)).await;

    match state.as_str() {
        "enabled" => track.wait_for_enabled().await.unwrap(),
        "disabled" => track.wait_for_disabled().await.unwrap(),
        _ => unreachable!(),
    };
}

#[then(regex = "^(\\S+) doesn't have (audio|(?:device|display) video) \
                 remote track from (\\S+)$")]
async fn then_doesnt_have_remote_track(
    world: &mut World,
    id: String,
    kind: String,
    partner_id: String,
) {
    let member = world.get_member(&id).unwrap();
    let partner_connection = member
        .connections()
        .wait_for_connection(partner_id)
        .await
        .unwrap();
    let tracks_with_partner = partner_connection.tracks_store().await.unwrap();
    let (media_kind, source_kind) = parse_media_kinds(&kind).unwrap();

    assert!(!tracks_with_partner
        .has_track(media_kind, Some(source_kind))
        .await
        .unwrap());
}

#[then(regex = r"^(\S+) doesn't have remote tracks from (\S+)$")]
async fn then_member_doesnt_have_remote_tracks_with(
    world: &mut World,
    id: String,
    partner_id: String,
) {
    let member = world.get_member(&id).unwrap();
    let connection = member
        .connections()
        .wait_for_connection(partner_id)
        .await
        .unwrap();
    let tracks_store = connection.tracks_store().await.unwrap();
    let tracks_count = tracks_store.count().await.unwrap();
    assert_eq!(tracks_count, 0);
}

#[then(regex = r"^(\S+) doesn't have live local tracks$")]
async fn then_member_doesnt_have_live_local_tracks(
    world: &mut World,
    id: String,
) {
    let member = world.get_member(&id).unwrap();
    let local_tracks = member.room().local_tracks().await.unwrap();
    let count = local_tracks.count_tracks_by_live(true).await.unwrap();
    assert_eq!(count, 0);
}

#[then(regex = r"^(\S+) has (\d+) (live|stopped) remote tracks from (\S+)$")]
async fn then_member_has_n_remote_tracks_from(
    world: &mut World,
    id: String,
    expected_count: u64,
    live_or_stopped: String,
    remote_id: String,
) {
    let member = world.get_member(&id).unwrap();
    let connection = member
        .connections()
        .wait_for_connection(remote_id)
        .await
        .unwrap();
    let tracks_store = connection.tracks_store().await.unwrap();
    let live = live_or_stopped == "live";

    let mut actual_count = 0;
    for _ in 0..5 {
        actual_count = tracks_store.count_tracks_by_live(live).await.unwrap();
        if actual_count != expected_count {
            sleep(Duration::from_millis(300)).await;
        }
    }

    assert_eq!(actual_count, expected_count);
}
