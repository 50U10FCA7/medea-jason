//! Wrapper around [RTCPeerConnection][1].
//!
//! [1]: https://w3.org/TR/webrtc#dom-rtcpeerconnection

#![allow(clippy::unwrap_used)]

use std::{
    cell::{Cell, RefCell},
    future::Future,
    rc::Rc,
};

use medea_client_api_proto::{
    IceConnectionState, IceServer, PeerConnectionState,
};
use tracerr::Traced;
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    Event, RtcBundlePolicy, RtcConfiguration, RtcIceCandidateInit,
    RtcIceConnectionState, RtcIceTransportPolicy, RtcOfferOptions,
    RtcPeerConnection as SysRtcPeerConnection, RtcPeerConnectionIceEvent,
    RtcRtpTransceiver, RtcRtpTransceiverInit, RtcSdpType,
    RtcSessionDescription, RtcSessionDescriptionInit, RtcTrackEvent,
};

use crate::{
    media::MediaKind,
    platform::{
        self,
        wasm::{get_property_by_name, utils::EventListener},
        IceCandidate, MediaStreamTrack, RtcPeerConnectionError, RtcStats,
        SdpType, Transceiver, TransceiverDirection,
    },
};

use super::ice_server::RtcIceServers;

/// Shortcut for a [`Result`] holding a [`Traced`] [`RtcPeerConnectionError`].
type RtcPeerConnectionResult<T> = Result<T, Traced<RtcPeerConnectionError>>;

/// Representation of [RTCPeerConnection][1].
///
/// [1]: https://w3.org/TR/webrtc#dom-rtcpeerconnection
#[derive(Debug)]
pub struct RtcPeerConnection {
    /// Underlying [RTCPeerConnection][1].
    ///
    /// [1]: https://w3.org/TR/webrtc/#rtcpeerconnection-interface
    peer: Rc<SysRtcPeerConnection>,

    /// Flag which indicates that ICE restart will be performed on next
    /// [`RtcPeerConnection::create_offer`] call.
    ice_restart: Cell<bool>,

    /// [`onicecandidate`][2] callback of [RTCPeerConnection][1] to handle
    /// [`icecandidate`][3] event. It fires when [RTCPeerConnection][1]
    /// discovers a new [RTCIceCandidate][4].
    ///
    /// [1]: https://w3.org/TR/webrtc/#rtcpeerconnection-interface
    /// [2]: https://w3.org/TR/webrtc#dom-rtcpeerconnection-onicecandidate
    /// [3]: https://w3.org/TR/webrtc/#event-icecandidate
    /// [4]: https://w3.org/TR/webrtc#dom-rtcicecandidate
    on_ice_candidate: RefCell<
        Option<EventListener<SysRtcPeerConnection, RtcPeerConnectionIceEvent>>,
    >,

    /// [`iceconnectionstatechange`][2] callback of [RTCPeerConnection][1],
    /// fires whenever [ICE connection state][3] changes.
    ///
    /// [1]: https://w3.org/TR/webrtc/#rtcpeerconnection-interface
    /// [2]: https://w3.org/TR/webrtc/#event-iceconnectionstatechange
    /// [3]: https://w3.org/TR/webrtc/#dfn-ice-connection-state
    on_ice_connection_state_changed:
        RefCell<Option<EventListener<SysRtcPeerConnection, Event>>>,

    /// [`connectionstatechange`][2] callback of [RTCPeerConnection][1],
    /// fires whenever the aggregate state of the connection changes.
    /// The aggregate state is a combination of the states of all individual
    /// network transports being used by the connection.
    ///
    /// Implemented in Chrome and Safari.
    /// Tracking issue for Firefox:
    /// <https://bugzilla.mozilla.org/show_bug.cgi?id=1265827>
    ///
    /// [1]: https://w3.org/TR/webrtc/#rtcpeerconnection-interface
    /// [2]: https://w3.org/TR/webrtc/#event-connectionstatechange
    on_connection_state_changed:
        RefCell<Option<EventListener<SysRtcPeerConnection, Event>>>,

    /// [`ontrack`][2] callback of [RTCPeerConnection][1] to handle
    /// [`track`][3] event. It fires when [RTCPeerConnection][1] receives
    /// new [MediaStreamTrack][4] from remote peer.
    ///
    /// [1]: https://w3.org/TR/webrtc/#rtcpeerconnection-interface
    /// [2]: https://w3.org/TR/webrtc#dom-rtcpeerconnection-ontrack
    /// [3]: https://w3.org/TR/webrtc/#event-track
    /// [4]: https://developer.mozilla.org/en-US/docs/Web/API/MediaStreamTrack
    on_track:
        RefCell<Option<EventListener<SysRtcPeerConnection, RtcTrackEvent>>>,
}

impl RtcPeerConnection {
    /// Instantiates new [`RtcPeerConnection`].
    ///
    /// # Errors
    ///
    /// Errors with [`RtcPeerConnectionError::PeerCreationError`] if
    /// [`SysRtcPeerConnection`] creation fails.
    #[allow(clippy::unused_async)] // for platform code uniformity
    pub async fn new<I>(
        ice_servers: I,
        is_force_relayed: bool,
    ) -> RtcPeerConnectionResult<Self>
    where
        I: IntoIterator<Item = IceServer>,
    {
        let mut peer_conf = RtcConfiguration::new();
        let policy = if is_force_relayed {
            RtcIceTransportPolicy::Relay
        } else {
            RtcIceTransportPolicy::All
        };
        _ = peer_conf.bundle_policy(RtcBundlePolicy::MaxBundle);
        _ = peer_conf.ice_transport_policy(policy);
        _ = peer_conf.ice_servers(&RtcIceServers::from(ice_servers));
        let peer = SysRtcPeerConnection::new_with_configuration(&peer_conf)
            .map_err(Into::into)
            .map_err(RtcPeerConnectionError::PeerCreationError)
            .map_err(tracerr::wrap!())?;

        Ok(Self {
            peer: Rc::new(peer),
            ice_restart: Cell::new(false),
            on_ice_candidate: RefCell::new(None),
            on_ice_connection_state_changed: RefCell::new(None),
            on_connection_state_changed: RefCell::new(None),
            on_track: RefCell::new(None),
        })
    }

    /// Returns [`RtcStats`] of this [`RtcPeerConnection`].
    ///
    /// # Errors
    ///
    /// Errors with [`RtcPeerConnectionError::RtcStatsError`] if getting or
    /// parsing of [`RtcStats`] fails.
    ///
    /// Errors with [`RtcPeerConnectionError::GetStatsException`] when
    /// [PeerConnection.getStats][1] promise throws exception.
    ///
    /// [1]: https://tinyurl.com/w6hmt5f
    pub async fn get_stats(&self) -> RtcPeerConnectionResult<RtcStats> {
        let js_stats =
            JsFuture::from(self.peer.get_stats()).await.map_err(|e| {
                tracerr::new!(RtcPeerConnectionError::GetStatsException(
                    platform::Error::from(e)
                ))
            })?;

        RtcStats::try_from(&js_stats).map_err(tracerr::map_from_and_wrap!())
    }

    /// Sets handler for a [`RtcTrackEvent`] (see [RTCTrackEvent][1] and
    /// [`ontrack` callback][2]).
    ///
    /// # Panics
    ///
    /// If binding to the [`track`][3] event fails. Not supposed to ever happen.
    ///
    /// [1]: https://w3.org/TR/webrtc/#rtctrackevent
    /// [2]: https://w3.org/TR/webrtc#dom-rtcpeerconnection-ontrack
    /// [3]: https://w3.org/TR/webrtc/#event-track
    pub fn on_track<F>(&self, f: Option<F>)
    where
        F: 'static + FnMut(MediaStreamTrack, Transceiver),
    {
        let mut on_track = self.on_track.borrow_mut();
        drop(match f {
            None => on_track.take(),
            Some(mut f) => {
                on_track.replace(
                    // Unwrapping is OK here, because this function shouldn't
                    // error ever.
                    EventListener::new_mut(
                        Rc::clone(&self.peer),
                        "track",
                        move |msg: RtcTrackEvent| {
                            f(
                                MediaStreamTrack::new(msg.track(), None),
                                Transceiver::from(msg.transceiver()),
                            );
                        },
                    )
                    .unwrap(),
                )
            }
        });
    }

    /// Sets handler for a [`RtcPeerConnectionIceEvent`] (see
    /// [RTCPeerConnectionIceEvent][1] and [`onicecandidate` callback][2]).
    ///
    /// # Panics
    ///
    /// If binding to the [`icecandidate`][3] event fails. Not supposed to ever
    /// happen.
    ///
    /// [1]: https://w3.org/TR/webrtc#dom-rtcpeerconnectioniceevent
    /// [2]: https://w3.org/TR/webrtc#dom-rtcpeerconnection-onicecandidate
    /// [3]: https://w3.org/TR/webrtc/#event-icecandidate
    pub fn on_ice_candidate<F>(&self, f: Option<F>)
    where
        F: 'static + FnMut(IceCandidate),
    {
        let mut on_ice_candidate = self.on_ice_candidate.borrow_mut();
        drop(match f {
            None => on_ice_candidate.take(),
            Some(mut f) => {
                on_ice_candidate.replace(
                    // Unwrapping is OK here, because this function shouldn't
                    // error ever.
                    EventListener::new_mut(
                        Rc::clone(&self.peer),
                        "icecandidate",
                        move |msg: RtcPeerConnectionIceEvent| {
                            // None candidate means that all ICE transports have
                            // finished gathering candidates.
                            // Doesn't need to be delivered onward to the remote
                            // peer.
                            if let Some(c) = msg.candidate() {
                                f(IceCandidate {
                                    candidate: c.candidate(),
                                    sdp_m_line_index: c.sdp_m_line_index(),
                                    sdp_mid: c.sdp_mid(),
                                });
                            }
                        },
                    )
                    .unwrap(),
                )
            }
        });
    }

    /// Returns [`RtcIceConnectionState`] of this [`RtcPeerConnection`].
    #[must_use]
    pub fn ice_connection_state(&self) -> IceConnectionState {
        parse_ice_connection_state(self.peer.ice_connection_state())
    }

    /// Returns [`PeerConnectionState`] of this [`RtcPeerConnection`].
    ///
    /// Returns [`None`] if failed to parse a [`PeerConnectionState`].
    #[must_use]
    pub fn connection_state(&self) -> Option<PeerConnectionState> {
        get_peer_connection_state(&self.peer)?.ok()
    }

    /// Sets handler for an [`iceconnectionstatechange`][1] event.
    ///
    /// # Panics
    ///
    /// If binding to the [`iceconnectionstatechange`][1] event fails. Not
    /// supposed to ever happen.
    ///
    /// [1]: https://w3.org/TR/webrtc/#event-iceconnectionstatechange
    pub fn on_ice_connection_state_change<F>(&self, f: Option<F>)
    where
        F: 'static + FnMut(IceConnectionState),
    {
        let mut on_ice_connection_state_changed =
            self.on_ice_connection_state_changed.borrow_mut();
        drop(match f {
            None => on_ice_connection_state_changed.take(),
            Some(mut f) => {
                let peer = Rc::clone(&self.peer);
                on_ice_connection_state_changed.replace(
                    // Unwrapping is OK here, because this function shouldn't
                    // error ever.
                    EventListener::new_mut(
                        Rc::clone(&self.peer),
                        "iceconnectionstatechange",
                        move |_| {
                            f(parse_ice_connection_state(
                                peer.ice_connection_state(),
                            ));
                        },
                    )
                    .unwrap(),
                )
            }
        });
    }

    /// Sets handler for a [`connectionstatechange`][1] event.
    ///
    /// # Panics
    ///
    /// If binding to the [`connectionstatechange`][1] event fails. Not supposed
    /// to ever happen.
    ///
    /// [1]: https://w3.org/TR/webrtc/#event-connectionstatechange
    pub fn on_connection_state_change<F>(&self, f: Option<F>)
    where
        F: 'static + FnMut(PeerConnectionState),
    {
        let mut on_connection_state_changed =
            self.on_connection_state_changed.borrow_mut();
        drop(match f {
            None => on_connection_state_changed.take(),
            Some(mut f) => {
                let peer = Rc::clone(&self.peer);
                on_connection_state_changed.replace(
                    // Unwrapping is OK here, because this function shouldn't
                    // error ever.
                    EventListener::new_mut(
                        Rc::clone(&self.peer),
                        "connectionstatechange",
                        move |_| {
                            // Error here should never happen, because if the
                            // browser does not support the functionality of
                            // `RTCPeerConnection.connectionState`, then this
                            // callback won't fire.
                            match get_peer_connection_state(&peer) {
                                Some(Ok(state)) => {
                                    f(state);
                                }
                                Some(Err(state)) => {
                                    log::error!(
                                        "Unknown RTCPeerConnection connection \
                                         state: {state}",
                                    );
                                }
                                None => {
                                    log::error!(
                                        "Could not receive RTCPeerConnection \
                                         connection state",
                                    );
                                }
                            }
                        },
                    )
                    .unwrap(),
                )
            }
        });
    }

    /// Adds remote [RTCPeerConnection][1]'s [ICE candidate][2] to this
    /// [`RtcPeerConnection`].
    ///
    /// # Errors
    ///
    /// With [`RtcPeerConnectionError::AddIceCandidateFailed`] if
    /// [RtcPeerConnection.addIceCandidate()][3] fails.
    ///
    /// [1]: https://w3.org/TR/webrtc/#rtcpeerconnection-interface
    /// [2]: https://tools.ietf.org/html/rfc5245#section-2
    /// [3]: https://w3.org/TR/webrtc#dom-peerconnection-addicecandidate
    pub async fn add_ice_candidate(
        &self,
        candidate: &str,
        sdp_m_line_index: Option<u16>,
        sdp_mid: &Option<String>,
    ) -> RtcPeerConnectionResult<()> {
        let mut cand_init = RtcIceCandidateInit::new(candidate);
        _ = cand_init
            .sdp_m_line_index(sdp_m_line_index)
            .sdp_mid(sdp_mid.as_ref().map(String::as_ref));
        JsFuture::from(
            self.peer.add_ice_candidate_with_opt_rtc_ice_candidate_init(
                Some(cand_init).as_ref(),
            ),
        )
        .await
        .map(drop)
        .map_err(Into::into)
        .map_err(RtcPeerConnectionError::AddIceCandidateFailed)
        .map_err(tracerr::wrap!())?;
        Ok(())
    }

    /// Marks [`RtcPeerConnection`] to trigger ICE restart.
    ///
    /// After this function returns, the offer returned by the next call to
    /// [`RtcPeerConnection::create_offer`] is automatically configured
    /// to trigger ICE restart.
    pub fn restart_ice(&self) {
        self.ice_restart.set(true);
    }

    /// Sets local description to the provided one [`RtcSdpType`].
    ///
    /// # Errors
    ///
    /// With [`RtcPeerConnectionError::SetLocalDescriptionFailed`] if
    /// [RtcPeerConnection.setLocalDescription()][1] fails.
    ///
    /// [1]: https://w3.org/TR/webrtc#dom-peerconnection-setlocaldescription
    async fn set_local_description(
        &self,
        sdp_type: RtcSdpType,
        offer: &str,
    ) -> RtcPeerConnectionResult<()> {
        let peer: Rc<SysRtcPeerConnection> = Rc::clone(&self.peer);

        let mut desc = RtcSessionDescriptionInit::new(sdp_type);
        _ = desc.sdp(offer);

        JsFuture::from(peer.set_local_description(&desc))
            .await
            .map(drop)
            .map_err(Into::into)
            .map_err(RtcPeerConnectionError::SetLocalDescriptionFailed)
            .map_err(tracerr::wrap!())?;

        Ok(())
    }

    /// Sets provided [SDP offer][`SdpType::Offer`] as local description.
    ///
    /// # Errors
    ///
    /// With [`RtcPeerConnectionError::SetLocalDescriptionFailed`] if
    /// [RtcPeerConnection.setLocalDescription()][1] fails.
    ///
    /// [1]: https://w3.org/TR/webrtc#dom-peerconnection-setlocaldescription
    pub async fn set_offer(&self, offer: &str) -> RtcPeerConnectionResult<()> {
        self.set_local_description(RtcSdpType::Offer, offer)
            .await
            .map_err(tracerr::map_from_and_wrap!())
    }

    /// Sets provided [SDP answer][`SdpType::Answer`] as local description.
    ///
    /// # Errors
    ///
    /// With [`RtcPeerConnectionError::SetLocalDescriptionFailed`] if
    /// [RtcPeerConnection.setLocalDescription()][1] fails.
    ///
    /// [1]: https://w3.org/TR/webrtc#dom-peerconnection-setlocaldescription
    pub async fn set_answer(
        &self,
        answer: &str,
    ) -> RtcPeerConnectionResult<()> {
        self.set_local_description(RtcSdpType::Answer, answer)
            .await
            .map_err(tracerr::map_from_and_wrap!())
    }

    /// Obtains [SDP answer][`SdpType::Answer`] from the underlying
    /// [RTCPeerConnection][`SysRtcPeerConnection`].
    ///
    /// Should be called whenever remote description has been changed.
    ///
    /// # Errors
    ///
    /// With [`RtcPeerConnectionError::CreateAnswerFailed`] if
    /// [RtcPeerConnection.createAnswer()][1] fails.
    ///
    /// [1]: https://w3.org/TR/webrtc#dom-rtcpeerconnection-createanswer
    pub async fn create_answer(&self) -> RtcPeerConnectionResult<String> {
        let answer = JsFuture::from(self.peer.create_answer())
            .await
            .map_err(Into::into)
            .map_err(RtcPeerConnectionError::CreateAnswerFailed)
            .map_err(tracerr::wrap!())?;
        let answer = RtcSessionDescription::from(answer).sdp();

        Ok(answer)
    }

    /// Rollbacks the underlying [RTCPeerConnection][`SysRtcPeerConnection`] to
    /// the previous stable state.
    ///
    /// # Errors
    ///
    /// With [`RtcPeerConnectionError::SetLocalDescriptionFailed`] if
    /// [RtcPeerConnection.setLocalDescription()][1] fails.
    ///
    /// [1]: https://w3.org/TR/webrtc#dom-peerconnection-setlocaldescription
    pub async fn rollback(&self) -> RtcPeerConnectionResult<()> {
        let peer: Rc<SysRtcPeerConnection> = Rc::clone(&self.peer);

        JsFuture::from(peer.set_local_description(
            &RtcSessionDescriptionInit::new(RtcSdpType::Rollback),
        ))
        .await
        .map(drop)
        .map_err(Into::into)
        .map_err(RtcPeerConnectionError::SetLocalDescriptionFailed)
        .map_err(tracerr::wrap!())?;

        Ok(())
    }

    /// Obtains [SDP offer][`SdpType::Offer`] from the underlying
    /// [RTCPeerConnection][`SysRtcPeerConnection`].
    ///
    /// Should be called after local tracks changes, which require
    /// (re)negotiation.
    ///
    /// # Errors
    ///
    /// With [`RtcPeerConnectionError::CreateOfferFailed`] if
    /// [RtcPeerConnection.createOffer()][1] fails.
    ///
    /// [1]: https://w3.org/TR/webrtc#dom-rtcpeerconnection-createoffer
    pub async fn create_offer(&self) -> RtcPeerConnectionResult<String> {
        let peer: Rc<SysRtcPeerConnection> = Rc::clone(&self.peer);

        let mut offer_options = RtcOfferOptions::new();
        if self.ice_restart.take() {
            _ = offer_options.ice_restart(true);
        }
        let create_offer = JsFuture::from(
            peer.create_offer_with_rtc_offer_options(&offer_options),
        )
        .await
        .map_err(Into::into)
        .map_err(RtcPeerConnectionError::CreateOfferFailed)
        .map_err(tracerr::wrap!())?;
        let offer = RtcSessionDescription::from(create_offer).sdp();

        Ok(offer)
    }

    /// Instructs the underlying [RTCPeerConnection][`SysRtcPeerConnection`]
    /// to apply the supplied [SDP][`SdpType`] as the remote
    /// [offer][`SdpType::Offer`] or [answer][`SdpType::Answer`].
    ///
    /// Changes the local media state.
    ///
    /// # Errors
    ///
    /// With [`RtcPeerConnectionError::SetRemoteDescriptionFailed`] if
    /// [RTCPeerConnection.setRemoteDescription()][1] fails.
    ///
    /// [1]: https://w3.org/TR/webrtc#dom-peerconnection-setremotedescription
    pub async fn set_remote_description(
        &self,
        sdp: SdpType,
    ) -> RtcPeerConnectionResult<()> {
        let description = match sdp {
            SdpType::Offer(offer) => {
                let mut desc =
                    RtcSessionDescriptionInit::new(RtcSdpType::Offer);
                _ = desc.sdp(&offer);
                desc
            }
            SdpType::Answer(answer) => {
                let mut desc =
                    RtcSessionDescriptionInit::new(RtcSdpType::Answer);
                _ = desc.sdp(&answer);
                desc
            }
        };

        JsFuture::from(self.peer.set_remote_description(&description))
            .await
            .map(drop)
            .map_err(Into::into)
            .map_err(RtcPeerConnectionError::SetRemoteDescriptionFailed)
            .map_err(tracerr::wrap!())?;

        Ok(())
    }

    /// Creates new [`RtcRtpTransceiver`] (see [RTCRtpTransceiver][1])
    /// and adds it to the [set of this RTCPeerConnection's transceivers][2].
    ///
    /// [1]: https://w3.org/TR/webrtc#dom-rtcrtptransceiver
    /// [2]: https://w3.org/TR/webrtc/#transceivers-set
    pub fn add_transceiver(
        &self,
        kind: MediaKind,
        direction: TransceiverDirection,
    ) -> impl Future<Output = Transceiver> + 'static {
        let peer = Rc::clone(&self.peer);
        async move {
            let mut init = RtcRtpTransceiverInit::new();
            _ = init.direction(direction.into());
            let transceiver =
                peer.add_transceiver_with_str_and_init(kind.as_str(), &init);
            Transceiver::from(transceiver)
        }
    }

    /// Returns [`RtcRtpTransceiver`] (see [RTCRtpTransceiver][1]) from a
    /// [set of this RTCPeerConnection's transceivers][2] by provided `mid`.
    ///
    /// # Panics
    ///
    /// If fails to [iterate over transceivers on JS side](js_sys::try_iter).
    /// Not supposed to ever happen.
    ///
    /// [1]: https://w3.org/TR/webrtc#dom-rtcrtptransceiver
    /// [2]: https://w3.org/TR/webrtc/#transceivers-set
    pub fn get_transceiver_by_mid(
        &self,
        mid: String,
    ) -> impl Future<Output = Option<Transceiver>> + 'static {
        let peer = Rc::clone(&self.peer);
        async move {
            let mut transceiver = None;

            let transceivers =
                js_sys::try_iter(&peer.get_transceivers()).unwrap().unwrap();
            for tr in transceivers {
                let tr = RtcRtpTransceiver::from(tr.unwrap());
                if let Some(tr_mid) = tr.mid() {
                    if mid.eq(&tr_mid) {
                        transceiver = Some(tr);
                        break;
                    }
                }
            }

            transceiver.map(Transceiver::from)
        }
    }
}

impl Drop for RtcPeerConnection {
    /// Drops [`on_track`][`RtcPeerConnection::on_track`] and
    /// [`on_ice_candidate`][`RtcPeerConnection::on_ice_candidate`] callbacks,
    /// and [closes][1] the underlying
    /// [RTCPeerConnection][`SysRtcPeerConnection`].
    ///
    /// [1]: https://w3.org/TR/webrtc#dom-rtcpeerconnection-close
    fn drop(&mut self) {
        drop(self.on_track.borrow_mut().take());
        drop(self.on_ice_candidate.borrow_mut().take());
        drop(self.on_ice_connection_state_changed.borrow_mut().take());
        drop(self.on_connection_state_changed.borrow_mut().take());
        self.peer.close();
    }
}

/// Returns [RTCPeerConnection.connectionState][1] property of provided
/// [`SysRtcPeerConnection`] using reflection.
///
/// [1]: https://w3.org/TR/webrtc#dom-peerconnection-connection-state
fn get_peer_connection_state(
    peer: &SysRtcPeerConnection,
) -> Option<Result<PeerConnectionState, String>> {
    let state =
        get_property_by_name(peer, "connectionState", |v| v.as_string())?;
    Some(Ok(match state.as_str() {
        "new" => PeerConnectionState::New,
        "connecting" => PeerConnectionState::Connecting,
        "connected" => PeerConnectionState::Connected,
        "disconnected" => PeerConnectionState::Disconnected,
        "failed" => PeerConnectionState::Failed,
        "closed" => PeerConnectionState::Closed,
        _ => {
            return Some(Err(state));
        }
    }))
}

/// Parses a [`IceConnectionState`] out of the given [`RtcIceConnectionState`].
fn parse_ice_connection_state(
    state: RtcIceConnectionState,
) -> IceConnectionState {
    use RtcIceConnectionState as S;

    match state {
        S::New => IceConnectionState::New,
        S::Checking => IceConnectionState::Checking,
        S::Connected => IceConnectionState::Connected,
        S::Completed => IceConnectionState::Completed,
        S::Failed => IceConnectionState::Failed,
        S::Disconnected => IceConnectionState::Disconnected,
        S::Closed => IceConnectionState::Closed,
        S::__Nonexhaustive => {
            unreachable!("Unknown ICE connection state {state:?}");
        }
    }
}
