use crate::model::{MediaServerInstance, PlayUrls};

pub fn build_play_urls(server: &MediaServerInstance, stream_id: &str) -> PlayUrls {
    let host = &server.public_host;
    PlayUrls {
        rtmp: format!("rtmp://{host}:{}/live/{stream_id}", server.rtmp_port),
        rtsp: format!("rtsp://{host}:{}/{stream_id}", server.rtsp_port),
        http_flv: Some(format!(
            "http://{host}:{}/flv/{stream_id}",
            server.http_port()
        )),
        hls: Some(format!(
            "http://{host}:{}/hls/{stream_id}/live.m3u8",
            server.http_port()
        )),
        webrtc_test_page: format!(
            "http://{host}:{}/webrtc/webrtc-test.html",
            server.http_port()
        ),
    }
}
