use crate::error::{Result, ServerError};

use core::fmt;
use libwebrtc::ffi::rtp_transceiver::{C_RtpTransceiverDirection, C_cricket_MediaType};
use libwebrtc::ffi::sdp::SdpType;
use libwebrtc::ffi::stats_collector::Rs_VideoSenderStats;
use libwebrtc::peerconnection::{
    IceServer, PeerConnection as WebRtcPeerConnection, RTCConfiguration,
};
use libwebrtc::peerconnection_factory::PeerConnectionFactory;
use libwebrtc::peerconnection_observer::{PeerConnectionObserver, PeerConnectionObserverTrait};
use libwebrtc::rtp_transceiver::RtpTransceiverInit;
use libwebrtc::rust_video_track_source::RustTrackVideoSource;
use libwebrtc::sdp::SessionDescription;
use libwebrtc::stats_collector::{DummyRTCStatsCollector, RTCStatsCollectorCallback};
use log::debug;
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc::{Receiver, Sender};
use tracing::{info, warn};

// TODO: temp allowing dead code, only used in tests currently
#[allow(dead_code)]
pub(crate) struct PeerConnection {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) webrtc_peer_connection: WebRtcPeerConnection,
    pub(crate) observer: PeerConnectionObserver,
    pub(crate) receiver: Receiver<String>,
}

impl fmt::Debug for PeerConnection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "id={}, name={}", self.id, self.name)
    }
}

#[derive(Clone)]
pub(crate) struct ChannelPeerConnectionObserver {
    pub(crate) sender: Sender<String>,
}

impl PeerConnectionObserverTrait for ChannelPeerConnectionObserver {
    fn on_ice_candidate(&mut self, candidate_sdp: &str, sdp_mid: &str, sdp_mline_index: u32) {
        info!("candidate generated: {} {}", sdp_mid, sdp_mline_index);

        if self.sender.blocking_send(candidate_sdp.to_owned()).is_err() {
            warn!("could not pass sdp candidate");
        }
    }
}

impl PeerConnection {
    pub(crate) fn new(
        peer_connection_factory: &PeerConnectionFactory,
        video_source: &RustTrackVideoSource,
        id: String,
        name: String,
    ) -> Result<PeerConnection> {
        debug!("Creating observer");
        let (tx, rx) = tokio::sync::mpsc::channel::<String>(10);
        let observer = PeerConnectionObserver::new(ChannelPeerConnectionObserver { sender: tx })
            .map_err(|e| ServerError::CreatePeerConnectionError(e.to_string()))?;
        debug!("created pc observer");

        let webrtc_peer_connection = peer_connection_factory
            .create_peer_connection(&observer, Self::rtc_config())
            .map_err(|e| ServerError::CreatePeerConnectionError(e.to_string()))?;
        debug!("created peerconnection");

        // add the video track
        peer_connection_factory.create_and_add_video_track(&webrtc_peer_connection, &video_source);

        Ok(PeerConnection {
            id,
            name,
            webrtc_peer_connection,
            observer,
            receiver: rx,
        })
    }

    fn rtc_config() -> RTCConfiguration {
        RTCConfiguration {
            enable_dtls_srtp: true,
            ice_servers: vec![IceServer {
                username: None,
                password: None,
                hostname: None,
                urls: vec!["stun:stun.l.google.com:19302".to_string()],
            }],
            ..Default::default()
        }
    }

    /// Send the callback to the rust ffi bindings and just listen for the first message.
    ///
    /// If the message fails, just return an empty vec.
    pub(crate) fn get_stats(&self) -> Vec<Rs_VideoSenderStats> {
        let (sender, receiver) = channel();
        let sender = Arc::new(Mutex::new(sender));
        let stats_collector = DummyRTCStatsCollector::new(sender);
        let stats_callback: RTCStatsCollectorCallback = stats_collector.into();
        let _ = self.webrtc_peer_connection.get_stats(&stats_callback);

        receiver.recv().unwrap_or_default()
    }

    pub(crate) fn create_offer(&mut self) -> Result<SessionDescription> {
        let sdp = self
            .webrtc_peer_connection
            .create_offer()
            .map_err(|e| ServerError::CouldNotCreateOffer(e.to_string()))?;

        Ok(sdp)
    }

    pub(crate) fn create_answer(&mut self) -> Result<SessionDescription> {
        let sdp = self
            .webrtc_peer_connection
            .create_answer()
            .map_err(|e| ServerError::CouldNotCreateAnswer(e.to_string()))?;

        Ok(sdp)
    }

    pub(crate) fn set_local_description(&mut self, sdp_type: SdpType, sdp: String) -> Result<()> {
        let description = SessionDescription::from_string(sdp_type, sdp)
            .map_err(|e| ServerError::CouldNotParseSdp(e.to_string()))?;
        self.webrtc_peer_connection
            .set_local_description(description)
            .map_err(|e| ServerError::CouldNotSetSdp(e.to_string()))?;

        Ok(())
    }

    pub(crate) fn set_remote_description(&mut self, sdp_type: SdpType, sdp: String) -> Result<()> {
        let description = SessionDescription::from_string(sdp_type, sdp)
            .map_err(|e| ServerError::CouldNotParseSdp(e.to_string()))?;
        self.webrtc_peer_connection
            .set_remote_description(description)
            .map_err(|e| ServerError::CouldNotSetSdp(e.to_string()))?;

        Ok(())
    }

    pub(crate) fn add_track(
        &self,
        peer_connection_factory: &PeerConnectionFactory,
        video_source: &RustTrackVideoSource,
        label: String,
    ) -> Result<bool> {
        debug!("creating video track");
        let track = peer_connection_factory
            .create_video_track(video_source, label)
            .map_err(|e| ServerError::CouldNotCreateTrack(e.to_string()))?;
        let stream_ids = vec!["0".to_owned()];

        debug!("adding video track to peer connection");
        let success = self.webrtc_peer_connection.add_track(track, stream_ids);

        Ok(success)
    }

    pub(crate) fn add_transceiver(
        &self,
        peer_connection_factory: &PeerConnectionFactory,
        video_source: &RustTrackVideoSource,
        label: String,
    ) -> Result<()> {
        let init = RtpTransceiverInit {
            direction: C_RtpTransceiverDirection::kSendRecv,
            stream_ids: vec!["0".to_owned()],
        };
        let track = peer_connection_factory
            .create_video_track(video_source, label)
            .map_err(|e| ServerError::CouldNotCreateTrack(e.to_string()))?;
        let stream_ids = vec!["0".to_owned()];
        self.webrtc_peer_connection
            .add_transceiver(track, init)
            .map_err(|e| ServerError::CouldNotAddTransceiver(e.to_string()))?;

        Ok(())
    }

    // stream a pre-encoded file from gstreamer to avoid encoding overhead
    pub(crate) fn file_video_source() -> RustTrackVideoSource {
        let video_source = RustTrackVideoSource::default();
        let (width, height) = (720, 480);
        video_source.start_gstreamer_thread_launch(
            & format!(
                "filesrc location=static/file.mp4 ! qtdemux name=demux demux.video_0 ! avdec_h264 ! videoconvert ! videoscale ! video/x-raw,format=I420,width={},height={}",
                width,
                height,
            ),
            width,
            height,
        );

        video_source
    }
}

#[cfg(test)]
pub(crate) mod tests {

    use super::*;
    use nanoid::nanoid;

    pub(crate) fn peer_connection_params() -> (PeerConnectionFactory, RustTrackVideoSource) {
        let factory = PeerConnectionFactory::new().unwrap();
        let video_source = PeerConnection::file_video_source();
        (factory, video_source)
    }

    #[tokio::test]
    async fn it_creates_a_new_peer_connection() {
        let (factory, video_source) = peer_connection_params();
        PeerConnection::new(&factory, &video_source, nanoid!(), "new".into()).unwrap();
    }

    #[tokio::test]
    async fn it_gets_stats_for_a_peer_connection() {
        let (factory, video_source) = peer_connection_params();
        let pc = PeerConnection::new(&factory, &video_source, nanoid!(), "new".into()).unwrap();
        pc.get_stats();
    }

    #[test]
    fn it_creates_an_offer() {
        let (factory, video_source) = peer_connection_params();
        let mut pc = PeerConnection::new(&factory, &video_source, nanoid!(), "new".into()).unwrap();
        pc.create_offer().unwrap();
    }

    #[test]
    fn it_creates_an_answer() {
        let (factory, video_source) = peer_connection_params();
        let mut pc = PeerConnection::new(&factory, &video_source, nanoid!(), "new".into()).unwrap();
        let offer = pc.create_offer().unwrap();
        pc.set_remote_description(offer.get_type().unwrap(), offer.to_string())
            .unwrap();
        pc.create_answer().unwrap();
    }

    #[test]
    fn it_sets_local_description() {
        let (factory, video_source) = peer_connection_params();
        let mut pc = PeerConnection::new(&factory, &video_source, nanoid!(), "new".into()).unwrap();
        let offer = pc.create_offer().unwrap();
        pc.set_local_description(offer.get_type().unwrap(), offer.to_string())
            .unwrap();
    }

    #[test]
    fn it_sets_remote_description() {
        let (factory, video_source) = peer_connection_params();
        let mut pc = PeerConnection::new(&factory, &video_source, nanoid!(), "new".into()).unwrap();
        let offer = pc.create_offer().unwrap();
        pc.set_remote_description(offer.get_type().unwrap(), offer.to_string())
            .unwrap();
    }

    #[test]
    fn it_adds_a_track() {
        let (factory, video_source) = peer_connection_params();
        let pc = PeerConnection::new(&factory, &video_source, nanoid!(), "new".into()).unwrap();
        pc.add_track(&factory, &video_source, "Testlabel".into())
            .unwrap();
    }

    #[test]
    fn it_adds_a_transceiver() {
        let (factory, video_source) = peer_connection_params();
        let pc = PeerConnection::new(&factory, &video_source, nanoid!(), "new".into()).unwrap();
        pc.add_transceiver(&factory, &video_source, "Testlabel".into())
            .unwrap();
    }

    // #[test]
    // fn it_does_all_the_things() {
    //     let (factory, video_source) = peer_connection_params();
    //     let pc = PeerConnection::new(&factory, &video_source, nanoid!(), "new".into()).unwrap();
    //     pc.add_transceiver().unwrap();
    // }
}
