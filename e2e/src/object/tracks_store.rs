//! Implementation and definition of store for the [`LocalTrack`]s and
//! [`RemoteTrack`]s.

use std::marker::PhantomData;

use crate::{
    browser::Statement,
    object::{
        local_track::LocalTrack,
        remote_track::RemoteTrack,
        room::{MediaKind, MediaSourceKind},
        Object,
    },
};

use super::Error;

/// Shortcut for a [`TracksStore`] of [`LocalTrack`]s.
pub type Local = TracksStore<LocalTrack>;

/// Shortcut for a [`TracksStore`] of [`RemoteTrack`]s.
pub type Remote = TracksStore<RemoteTrack>;

/// Store for [`LocalTrack`]s or [`RemoteTrack`]s.
#[derive(Debug)]
pub struct TracksStore<T>(PhantomData<T>);

impl<T> Object<TracksStore<T>> {
    /// Returns count of tracks stored in this [`TracksStore`].
    ///
    /// # Errors
    ///
    /// - If failed to execute JS statement.
    /// - If failed to parse result as [`u64`].
    pub async fn count(&self) -> Result<u64, Error> {
        self.execute(Statement::new(
            // language=JavaScript
            "async (store) => store.tracks.length",
            [],
        ))
        .await?
        .as_u64()
        .ok_or(Error::TypeCast)
    }

    /// Waits this [`TracksStore`] to contain `count` tracks.
    ///
    /// # Errors
    ///
    /// If failed to execute JS statement.
    pub async fn wait_for_count(&self, count: u64) -> Result<(), Error> {
        if count == 0 {
            return Ok(());
        }

        self.execute(Statement::new(
            // language=JavaScript
            "
            async (store) => {
                const [neededCount] = args;
                let currentCount = store.tracks.length;
                if (currentCount === neededCount) {
                    return;
                } else {
                    let waiter = new Promise((resolve) => {
                        store.subs.push(() => {
                            currentCount += 1;
                            if (currentCount === neededCount) {
                                resolve();
                                return false;
                            }
                            return true;
                        });
                    });
                    await waiter;
                }
            }
            ",
            [count.into()],
        ))
        .await
        .map(drop)
    }

    /// Indicates whether this [`TracksStore`] contains a track with the
    /// provided [`MediaKind`] and [`MediaSourceKind`].
    ///
    /// # Errors
    ///
    /// - If failed to execute JS statement.
    /// - If failed to parse result as [`bool`].
    pub async fn has_track(
        &self,
        kind: MediaKind,
        source_kind: Option<MediaSourceKind>,
    ) -> Result<bool, Error> {
        let source_kind_js =
            source_kind.map_or("undefined", MediaSourceKind::as_js);
        let kind_js = Statement::new(
            // language=JavaScript
            &format!(
                r#"
                async (store) => {{
                    return {{
                        store: store,
                        kind: {kind},
                        sourceKind: {source_kind_js}
                    }};
                }}
                "#,
                kind = kind.as_js()
            ),
            [],
        );

        self.execute(kind_js.and_then(Statement::new(
            // language=JavaScript
            "
            async (meta) => {
                for (track of meta.store.tracks) {
                    if (track.track.kind() === meta.kind &&
                        (
                            track.track.media_source_kind()  ===
                            meta.sourceKind ||
                            meta.sourceKind === undefined
                        )
                    ) {
                        return true;
                    }
                }
                return false;
             }
            ",
            [],
        )))
        .await?
        .as_bool()
        .ok_or(Error::TypeCast)
    }

    /// Returns a track from this [`TracksStore`] with the provided
    /// [`MediaKind`] and [`MediaSourceKind`].
    ///
    /// # Errors
    ///
    /// If failed to execute JS statement.
    pub async fn get_track(
        &self,
        kind: MediaKind,
        source_kind: MediaSourceKind,
    ) -> Result<Object<T>, Error> {
        let kind_js = Statement::new(
            // language=JavaScript
            &format!(
                r#"
                async (store) => {{
                    return {{
                        store: store,
                        kind: {kind},
                        sourceKind: {source_kind}
                    }};
                }}
                "#,
                source_kind = source_kind.as_js(),
                kind = kind.as_js()
            ),
            [],
        );

        self.execute_and_fetch(kind_js.and_then(Statement::new(
            // language=JavaScript
            "
            async (meta) => {
                for (track of meta.store.tracks) {
                    let kind = track.track.kind();
                    let sourceKind = track.track.media_source_kind();
                    if (kind === meta.kind
                        && sourceKind === meta.sourceKind) {
                        return track;
                    }
                }
                let waiter = new Promise((resolve) => {
                    meta.store.subs.push((track) => {
                        let kind = track.track.kind();
                        let sourceKind =
                            track.track.media_source_kind();
                        if (kind === meta.kind
                            && sourceKind === meta.sourceKind) {
                            resolve(track);
                            return false;
                        } else {
                            return true;
                        }
                    });
                });
                return await waiter;
            }
            ",
            [],
        )))
        .await
    }

    /// Returns count of tracks by the provided `live` values.
    ///
    /// # Errors
    ///
    /// - If failed to execute JS statement.
    /// - If failed to parse result as [`u64`].
    pub async fn count_tracks_by_live(&self, live: bool) -> Result<u64, Error> {
        self.execute(Statement::new(
            // language=JavaScript
            &format!(
                r#"
                async (store) => {{
                    let count = 0;
                    for (track of store.tracks) {{
                        if ({live} && !track.stopped) {{
                            count++;
                        }} else if (!{live} && track.stopped) {{
                            count++;
                        }}
                    }}
                    return count;
                }}
                "#,
            ),
            [],
        ))
        .await?
        .as_u64()
        .ok_or(Error::TypeCast)
    }
}
