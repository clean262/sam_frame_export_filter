use aviutl2::{
    AnyResult,
    filter::{
        FilterConfigItems,
        FilterConfigItem,
        FilterPlugin,
        FilterPluginTable,
        FilterProcVideo,
        RgbaPixel,
    },
};
use chrono::{Datelike, Local, Timelike};
use env_logger::{Builder, Env, Target};
use std::{
    collections::HashMap,
    fs::{File, create_dir_all, read, write},
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::PathBuf,
    process::Command,
    sync::{Mutex, Once, OnceLock},
    thread,
};

/// ロガー初期化（1プロセスにつき1回）
#[cfg(debug_assertions)]
fn init_logger() {
    static INIT: Once = Once::new();

    INIT.call_once(|| {
        let log_dir = r"C:\ProgramData\aviutl2\Log";

        if let Err(e) = create_dir_all(log_dir) {
            eprintln!("failed to create log directory {}: {e}", log_dir);
            return;
        }

        let now = Local::now();
        let filename = format!(
            "sam_frame_export_{:04}_{:02}_{:02}_{:02}_{:02}.log",
            now.year(),
            now.month(),
            now.day(),
            now.hour(),
            now.minute(),
        );
        let log_path = format!(r"{}\{}", log_dir, filename);

        let file = match File::create(&log_path) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("failed to create log file at {}: {e}", log_path);
                return;
            }
        };

        let _ = Builder::from_env(Env::default().default_filter_or("debug"))
            .target(Target::Pipe(Box::new(file)))
            .try_init();
    });
}

#[cfg(not(debug_assertions))]
fn init_logger() {
    // リリース版では何もしない（ログファイルも作らない）
}


/// フィルタの設定項目。
///
/// run_sam: このフレームを SAM で前景抽出
/// output_file の親ディレクトリを保存先ルートとして使う。
#[derive(Debug, Clone, PartialEq, FilterConfigItems)]
struct FilterConfig {
    #[check(
        name = "※ ブラウザでhttp://127.0.0.1:17860/を開いて下さい",
        default = false
    )]
    _hint_open_web_ui: bool,

    #[check(name = "このフレームを SAM で前景抽出", default = false)]
    run_sam: bool,

    #[file(
        name = "保存先フォルダ内の任意ファイル",
        filters = {
            "すべてのファイル" => [],
        }
    )]
    output_file: Option<PathBuf>,
}

/// デフォルトの出力先 (AviUtl2 標準の Export フォルダ)
const EXPORT_DIR: &str = r"C:\ProgramData\aviutl2\Export";
/// 現在の保存ルートディレクトリ
/// 既定値: EXPORT_DIR
/// ユーザーが #[file] で何かファイルを選んだら、その親ディレクトリに更新
fn export_root_dir() -> &'static Mutex<PathBuf> {
    static EXPORT_ROOT_DIR: OnceLock<Mutex<PathBuf>> = OnceLock::new();
    EXPORT_ROOT_DIR.get_or_init(|| Mutex::new(PathBuf::from(EXPORT_DIR)))
}

/// プロジェクト内でSAMで切り抜いた背景の保存先を統一する
fn update_export_root_from_config(config: &FilterConfig) {
    if let Some(selected) = &config.output_file {
        if let Some(parent) = selected.parent() {
            let mut root = export_root_dir().lock().unwrap();
            *root = parent.to_path_buf();
            log::info!("Export root changed to {}", root.display());
        }
    }
}

/// SAMの起動を確かめるグローバルなオブジェクト状態テーブル
fn object_states() -> &'static Mutex<HashMap<i64, ObjectState>> {
    static STATES: OnceLock<Mutex<HashMap<i64, ObjectState>>> = OnceLock::new();
    STATES.get_or_init(|| Mutex::new(HashMap::new()))
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ObjectState {
    last_run_sam: bool,
}

/// Web UI のルートディレクトリ
const WEB_ROOT: &str =
    r"C:\ProgramData\aviutl2\Plugin\sam_frame_export_filter\web";


#[aviutl2::plugin(FilterPlugin)]
struct SamFrameExportFilter;

impl FilterPlugin for SamFrameExportFilter {
    /// コンストラクタ
    fn new(_info: aviutl2::AviUtl2Info) -> AnyResult<Self> {
        init_logger();
        log::info!("SamFrameExportFilter::new - plugin initialized");
        Ok(Self)
    }

    fn plugin_info(&self) -> FilterPluginTable {
        FilterPluginTable {
            name: "SAM Frame Export (PNG)".to_string(),
            label: Some("抽出".to_string()),
            information: format!(
                "SAM frame export filter v{} by cleaning (https://github.com/clean262/sam_frame_export_filter)",
                env!("CARGO_PKG_VERSION")
            ),
            filter_type: aviutl2::filter::FilterType::Video,
            as_object: false,
            config_items: FilterConfig::to_config_items(),
        }
    }


    fn proc_video(
        &self,
        config_items: &[FilterConfigItem],
        video: &mut FilterProcVideo,
    ) -> AnyResult<()> {
        log::debug!("SamFrameExportFilter::proc_video - start");

        let config = FilterConfig::from_config_items(config_items);

        update_export_root_from_config(&config);

        let object_id = video.object.id; // ObjectInfo.id (i64)

        // 編集中オブジェクト ID を更新
        {
        let mut edit = current_edit_object_id().lock().unwrap();
        *edit = Some(object_id);
        }

        // ── オブジェクトごとの run_sam の立ち上がりを検出 ──
        // run_sam チェックを入れた瞬間のフレームだけ should_export == trueになる
        let should_export = {
            let states_mutex = object_states();
            let mut states = states_mutex.lock().unwrap();
            let state = states
                .entry(object_id)
                .or_insert(ObjectState { last_run_sam: false });

            let rising_edge = config.run_sam && !state.last_run_sam;
            state.last_run_sam = config.run_sam;
            rising_edge // Should exportの返り値
        };

        // 立ち上がりのときだけ current_frame.png を書き出し、
        // Web UI を起動する。
        if should_export {
            log::info!(
                "SamFrameExportFilter::proc_video - run_sam triggered for object id {}",
                object_id
            );

            // 1) 現在フレームを RGBA で取得
            let (width, height, rgba_bytes) = get_rgba_frame_from_video(video)?;

            log::debug!(
                "SamFrameExportFilter::proc_video - frame size: {}x{} ({} bytes)",
                width,
                height,
                rgba_bytes.len()
            );

            let img = image::RgbaImage::from_vec(width, height, rgba_bytes)
                .ok_or_else(|| anyhow::anyhow!("RGBA buffer size mismatch: {}x{}", width, height))?;

            // 2) 固定ファイル名 current_frame.png に上書き保存
            let png_path = current_frame_png_path()?;
            log::info!(
                "SamFrameExportFilter::proc_video - saving PNG to {}",
                png_path.display()
            );
            img.save(&png_path)?;

            log::info!("SamFrameExportFilter::proc_video - PNG saved");

            // 3) HTTP サーバーとブラウザを起動
            start_http_server_once();
            open_browser_once();
        }

        // マスクは AviUtl2 に適用しない

        log::debug!("SamFrameExportFilter::proc_video - end");
        Ok(())
    }
}

impl Drop for SamFrameExportFilter {
    fn drop(&mut self) {
        log::info!("SamFrameExportFilter::drop - plugin dropped");
    }
}

// Aviutl2 プラグイン登録マクロ
aviutl2::register_filter_plugin!(SamFrameExportFilter);


/// 保存ルート配下の `current_frame.png` を返す。
fn current_frame_png_path() -> AnyResult<PathBuf> {
    let root = export_root_dir().lock().unwrap().clone();
    create_dir_all(&root)?;
    Ok(root.join("current_frame.png"))
}

/// 保存ルート配下にユニークなマスク PNG ファイルパスを作成する。
fn make_unique_mask_path() -> AnyResult<PathBuf> {
    let root = export_root_dir().lock().unwrap().clone();
    create_dir_all(&root)?;

    let now = Local::now();
    let base = format!(
        "sam_mask_{:04}{:02}{:02}_{:02}{:02}{:02}_{:03}",
        now.year(),
        now.month(),
        now.day(),
        now.hour(),
        now.minute(),
        now.second(),
        now.timestamp_subsec_millis(),
    );

    // sam_mask_YYYYMMDD_HHMMSS_mmm.png
    let mut filename = format!("{base}.png");
    let mut path = root.join(&filename);

    // もし同名ファイルがすでに存在していたら、_1, _2... を付けてずらす
    let mut counter = 1;
    while path.exists() {
        filename = format!("{base}_{counter}.png");
        path = root.join(&filename);
        counter += 1;
    }

    Ok(path)
}

/// FilterProcVideo から RGBA8 のフレームを取り出すためのヘルパー。
fn get_rgba_frame_from_video(
    video: &mut FilterProcVideo,
) -> AnyResult<(u32, u32, Vec<u8>)> {
    let width = video.video_object.width.max(0) as u32;
    let height = video.video_object.height.max(0) as u32;

    let num_pixels = (width * height) as usize;
    log::debug!(
        "get_rgba_frame_from_video - video_object size: {}x{} ({} pixels)",
        width,
        height,
        num_pixels
    );

    let mut pixels = vec![
        RgbaPixel {
            r: 0,
            g: 0,
            b: 0,
            a: 0,
        };
        num_pixels
    ];

    let written = video.get_image_data(&mut pixels[..]);

    if written != num_pixels {
        log::warn!(
            "get_image_data wrote {} pixels, expected {} ({}x{})",
            written,
            num_pixels,
            width,
            height
        );
    }

    let mut rgba_bytes = Vec::with_capacity(num_pixels * 4); // [R, G, B, A, R, G, B, A, ...]
    for p in &pixels {
        rgba_bytes.push(p.r);
        rgba_bytes.push(p.g);
        rgba_bytes.push(p.b);
        rgba_bytes.push(p.a);
    }

    Ok((width, height, rgba_bytes))
}

/// object_id → マスク PNG のフルパス
fn mask_paths() -> &'static Mutex<HashMap<i64, PathBuf>> {
    static MASK_PATHS: OnceLock<Mutex<HashMap<i64, PathBuf>>> = OnceLock::new();
    MASK_PATHS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn set_mask_path_for_object(object_id: i64, path: PathBuf) {
    let mut map = mask_paths().lock().unwrap();
    map.insert(object_id, path);
}

// ── ローカル HTTP サーバー ─────────────────────────────────────────────

/// HTTP サーバーを 1 度だけ起動する。
fn start_http_server_once() {
    static START: Once = Once::new();

    START.call_once(|| {
        log::info!("Starting local HTTP server thread...");

        thread::spawn(|| {
            if let Err(e) = run_http_server() {
                log::error!("HTTP server error: {e:?}");
            }
        });
    });
}

/// ブラウザを 1 度だけ起動する。
fn open_browser_once() {
    static OPEN: Once = Once::new();

    OPEN.call_once(|| {
        let url = "http://127.0.0.1:17860/";
        log::info!("Opening browser: {}", url);

        // Windows の既定ブラウザで URL を開く
        // start "" "URL"
        let result = Command::new("cmd")
            .args(&["/C", "start", "", url])
            .spawn();

        if let Err(e) = result {
            log::error!("Failed to open browser: {e:?}");
        }
    });
}

/// シンプルなローカル HTTP サーバー。
///
/// - 127.0.0.1:17860 で待ち受け
/// - GET /frame/current.png に current_frame.png を返す
/// - GET /, /index.html, /index.js, /index.css などに WEB_ROOT から静的ファイルを返す
/// - POST /mask に「SAM で切り抜かれた PNG（前景のみ）」が飛んでくるので、それを保存する
fn run_http_server() -> AnyResult<()> {
    let addr = "127.0.0.1:17860";
    let listener = TcpListener::bind(addr)?;
    log::info!("HTTP server listening on http://{addr}");

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                if let Err(e) = handle_client(stream) {
                    log::warn!("HTTP client error: {e:?}");
                }
            }
            Err(e) => {
                log::warn!("HTTP incoming error: {e:?}");
            }
        }
    }

    Ok(())
}

/// ヘッダ末尾 "\r\n\r\n" の位置を探す。
fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

fn handle_client(mut stream: TcpStream) -> AnyResult<()> {
    // 1. リクエスト全体（ヘッダ＋ボディ）をバッファに読み込む
    let mut buffer = Vec::new();
    let mut temp = [0u8; 4096];
    let mut header_end_pos: Option<usize> = None;

    loop {
        let n = stream.read(&mut temp)?;
        if n == 0 {
            break;
        }
        buffer.extend_from_slice(&temp[..n]);

        if header_end_pos.is_none() {
            if let Some(pos) = find_header_end(&buffer) {
                header_end_pos = Some(pos);
                break;
            }
        }

        if buffer.len() > 16 * 1024 {
            // ヘッダが異常に大きいのは想定外なので切る
            return Err(anyhow::anyhow!("HTTP header too large"));
        }
    }

    if buffer.is_empty() {
        return Ok(());
    }

    let header_end = header_end_pos
        .or_else(|| find_header_end(&buffer))
        .unwrap_or(buffer.len());
    let body_start = header_end + 4; // "\r\n\r\n" の分

    let header_bytes = &buffer[..header_end];
    let header_str = String::from_utf8_lossy(header_bytes);
    let mut lines = header_str.lines();

    let request_line = lines.next().unwrap_or("");
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("");
    let raw_path = parts.next().unwrap_or("/");
    let path = raw_path.split('?').next().unwrap_or("/");

    // Content-Length を取得（POST /mask 用）
    let mut content_length: usize = 0;
    for line in lines {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix("Content-Length:") {
            content_length = rest.trim().parse().unwrap_or(0);
        } else if let Some(rest) = line.strip_prefix("content-length:") {
            content_length = rest.trim().parse().unwrap_or(0);
        }
    }

    log::debug!("HTTP request: {} {}", method, path);

    // 2. ボディを取得（必要な場合）
    let mut body = Vec::new();
    if buffer.len() > body_start {
        body.extend_from_slice(&buffer[body_start..]);
    }

    // 必要に応じて Content-Length まで読み足す
    while body.len() < content_length {
        let n = stream.read(&mut temp)?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&temp[..n]);
    }

    // 3. メソッドとパスに応じて処理
    match method {
        "GET" => handle_get(&mut stream, path),
        "POST" => handle_post(&mut stream, path, &body),
        _ => {
            write_response(
                &mut stream,
                405,
                "Method Not Allowed",
                b"Method Not Allowed",
                "text/plain",
            )
        }
    }
}

/// GET リクエストの処理。
fn handle_get(stream: &mut TcpStream, path: &str) -> AnyResult<()> {
    if path == "/frame/current.png" {
        let path = current_frame_png_path()?;
        match read(&path) {
            Ok(data) => {
                write_response(
                    stream,
                    200,
                    "OK",
                    &data,
                    "image/png",
                )?;
            }
            Err(_) => {
                write_response(
                    stream,
                    404,
                    "Not Found",
                    b"current_frame.png not found",
                    "text/plain",
                )?;
            }
        }
        return Ok(());
    }

    // それ以外は WEB_ROOT から静的ファイルとして探す
    match serve_static_file(path) {
        Ok((body, content_type)) => {
            write_response(
                stream,
                200,
                "OK",
                &body,
                content_type,
            )?;
        }
        Err(e) => {
            log::debug!("Static file not found for {}: {:?}", path, e);
            write_response(
                stream,
                404,
                "Not Found",
                b"Not Found",
                "text/plain",
            )?;
        }
    }

    Ok(())
}

fn current_edit_object_id() -> &'static Mutex<Option<i64>> {
    static EDIT_ID: OnceLock<Mutex<Option<i64>>> = OnceLock::new();
    EDIT_ID.get_or_init(|| Mutex::new(None))
}

/// POST リクエストの処理。
///
/// `/mask` = 「SAMで切り抜かれた PNG を保存するだけ」
fn handle_post(stream: &mut TcpStream, path: &str, body: &[u8]) -> AnyResult<()> {
    if path == "/mask" {
        // 現在編集中のオブジェクト ID を取得（これは「どのオブジェクトのマスクか」を
        // マップに紐づけるためだけに使う。ファイル名には一切使わない）
        let object_id_opt = {
            let edit = current_edit_object_id().lock().unwrap();
            *edit
        };

        if let Some(object_id) = object_id_opt {
            let mask_path = make_unique_mask_path()?;
            log::info!(
                "Saving mask PNG for object {} to {} ({} bytes)",
                object_id,
                mask_path.display(),
                body.len()
            );

            write(&mask_path, body)?;

            // object_id → このファイルパス に紐づけ
            set_mask_path_for_object(object_id, mask_path.clone());

            write_response(stream, 200, "OK", b"OK", "text/plain")?;
        } else {
            log::warn!("POST /mask called but no current editing object id set");
            write_response(
                stream,
                400,
                "Bad Request",
                b"No editing object",
                "text/plain",
            )?;
        }
        return Ok(());
    }

    // 未対応パス
    write_response(
        stream,
        404,
        "Not Found",
        b"Not Found",
        "text/plain",
    )?;
    Ok(())
}

/// 静的ファイルを WEB_ROOT から返すヘルパー。
///
/// path: "/index.html", "/index.js", "/" など
fn serve_static_file(path: &str) -> AnyResult<(Vec<u8>, &'static str)> {
    // "/" → "index.html"
    let rel = if path == "/" || path.is_empty() {
        "index.html"
    } else {
        path.trim_start_matches('/')
    };

    // 簡易的なパストラバーサル防止
    if rel.contains("..") {
        return Err(anyhow::anyhow!("invalid path"));
    }

    let full_path = PathBuf::from(WEB_ROOT).join(rel);
    log::debug!("Serving static file: {}", full_path.display());

    let data = read(&full_path)?;

    let content_type = if rel.ends_with(".html") {
        "text/html; charset=utf-8"
    } else if rel.ends_with(".js") {
        "text/javascript; charset=utf-8"
    } else if rel.ends_with(".css") {
        "text/css; charset=utf-8"
    } else if rel.ends_with(".png") {
        "image/png"
    } else {
        "application/octet-stream"
    };

    Ok((data, content_type))
}

fn write_response(
    stream: &mut TcpStream,
    status_code: u16,
    reason: &str,
    body: &[u8],
    content_type: &str,
) -> AnyResult<()> {
    let header = format!(
        "HTTP/1.1 {} {}\r\n\
         Content-Type: {}\r\n\
         Content-Length: {}\r\n\
         Access-Control-Allow-Origin: *\r\n\
         Connection: close\r\n\
         \r\n",
        status_code,
        reason,
        content_type,
        body.len()
    );

    stream.write_all(header.as_bytes())?;
    stream.write_all(body)?;
    stream.flush()?;
    Ok(())
}
