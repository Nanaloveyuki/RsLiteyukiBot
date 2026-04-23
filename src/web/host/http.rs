use super::*;

#[derive(Debug, Clone)]
pub(super) struct MultipartField {
    pub name: String,
    pub filename: Option<String>,
    pub data: Vec<u8>,
}

pub(super) fn napcat_ok<T: Serialize>(data: &T) -> Vec<u8> {
    #[derive(Serialize)]
    struct Envelope<'a, T: Serialize> {
        code: i32,
        message: &'a str,
        data: &'a T,
    }
    serde_json::to_vec(&Envelope {
        code: 0,
        message: "ok",
        data,
    })
    .unwrap_or_else(|_| br#"{"code":0,"message":"ok","data":null}"#.to_vec())
}

pub(super) fn napcat_err(code: i32, message: &str) -> Vec<u8> {
    #[derive(Serialize)]
    struct Envelope<'a> {
        code: i32,
        message: &'a str,
        data: Option<()>,
    }
    serde_json::to_vec(&Envelope {
        code,
        message,
        data: None,
    })
    .unwrap_or_else(|_| br#"{"code":-1,"message":"error","data":null}"#.to_vec())
}

pub(super) fn extract_header<'a>(request: &'a str, name: &str) -> Option<&'a str> {
    request
        .lines()
        .find_map(|line| parse_named_header(line, name))
}

#[allow(dead_code)]
pub(super) fn extract_body(request: &[u8]) -> &[u8] {
    if let Some(pos) = request.windows(4).position(|w| w == b"\r\n\r\n") {
        &request[pos + 4..]
    } else {
        b""
    }
}

#[allow(dead_code)]
pub(super) fn parse_json_body(request: &[u8]) -> serde_json::Value {
    let body = extract_body(request);
    serde_json::from_slice(body).unwrap_or(serde_json::Value::Null)
}

pub(super) fn napcat_response(body: Vec<u8>, head_only: bool) -> Vec<u8> {
    build_response(
        "200 OK",
        "application/json; charset=utf-8",
        &body,
        head_only,
    )
}

pub(super) fn options_response() -> Vec<u8> {
    let headers = "HTTP/1.1 204 No Content\r\n\
        Access-Control-Allow-Origin: *\r\n\
        Access-Control-Allow-Methods: GET, POST, PUT, DELETE, OPTIONS\r\n\
        Access-Control-Allow-Headers: Authorization, Content-Type, Accept\r\n\
        Content-Length: 0\r\n\
        Connection: close\r\n\r\n";
    headers.as_bytes().to_vec()
}

pub(super) fn sse_response(event_data: &str, head_only: bool) -> Vec<u8> {
    let body = format!("data: {event_data}\n\n");
    build_response(
        "200 OK",
        "text/event-stream; charset=utf-8",
        body.as_bytes(),
        head_only,
    )
}

pub(super) fn sse_batch_response(events: &[String], head_only: bool) -> Vec<u8> {
    let body = events
        .iter()
        .map(|event| format!("data: {event}\n\n"))
        .collect::<String>();
    build_response(
        "200 OK",
        "text/event-stream; charset=utf-8",
        body.as_bytes(),
        head_only,
    )
}

pub(super) fn parse_query_string(raw_path: &str) -> HashMap<String, String> {
    let Some((_, query)) = raw_path.split_once('?') else {
        return HashMap::new();
    };

    query
        .split('&')
        .filter(|segment| !segment.trim().is_empty())
        .filter_map(|segment| {
            let (key, value) = segment.split_once('=').unwrap_or((segment, ""));
            Some((percent_decode(key)?, percent_decode(value)?))
        })
        .collect()
}

pub(super) fn parse_multipart_form_data(request: &[u8]) -> Result<Vec<MultipartField>, String> {
    let headers_end = request
        .windows(4)
        .position(|slice| slice == b"\r\n\r\n")
        .ok_or_else(|| "missing request headers".to_string())?;
    let header_text = String::from_utf8_lossy(&request[..headers_end + 4]);
    let content_type = extract_header(&header_text, "Content-Type")
        .ok_or_else(|| "missing Content-Type header".to_string())?;
    let boundary = parse_multipart_boundary(content_type)
        .ok_or_else(|| "missing multipart boundary".to_string())?;
    let marker = format!("--{boundary}").into_bytes();
    let body = &request[headers_end + 4..];
    let positions = find_all_subslices(body, &marker);
    if positions.len() < 2 || positions[0] != 0 {
        return Err("malformed multipart payload".to_string());
    }

    let mut fields = Vec::new();
    for window in positions.windows(2) {
        let mut part = &body[window[0] + marker.len()..window[1]];
        if part.starts_with(b"--") {
            break;
        }
        if part.starts_with(b"\r\n") {
            part = &part[2..];
        }
        if part.ends_with(b"\r\n") {
            part = &part[..part.len() - 2];
        }
        if part.is_empty() {
            continue;
        }

        let header_end = find_subslice(part, b"\r\n\r\n")
            .ok_or_else(|| "malformed multipart part".to_string())?;
        let headers = String::from_utf8_lossy(&part[..header_end]);
        let disposition = headers
            .lines()
            .find_map(|line| parse_named_header(line, "Content-Disposition"))
            .ok_or_else(|| "missing multipart Content-Disposition header".to_string())?;
        let name = parse_content_disposition_param(disposition, "name")
            .ok_or_else(|| "multipart field name is required".to_string())?;
        let filename = parse_content_disposition_param(disposition, "filename")
            .filter(|value| !value.trim().is_empty());
        let data = part[header_end + 4..].to_vec();
        fields.push(MultipartField {
            name,
            filename,
            data,
        });
    }

    Ok(fields)
}

fn percent_decode(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => {
                output.push(b' ');
                index += 1;
            }
            b'%' if index + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).ok()?;
                let decoded = u8::from_str_radix(hex, 16).ok()?;
                output.push(decoded);
                index += 3;
            }
            byte => {
                output.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8(output).ok()
}

fn parse_multipart_boundary(content_type: &str) -> Option<String> {
    content_type
        .split(';')
        .map(str::trim)
        .find_map(|segment| {
            let value = segment.strip_prefix("boundary=")?;
            Some(trim_http_quoted_string(value))
        })
        .filter(|value| !value.is_empty())
}

fn parse_content_disposition_param(disposition: &str, name: &str) -> Option<String> {
    disposition.split(';').map(str::trim).find_map(|segment| {
        let value = segment.strip_prefix(&format!("{name}="))?;
        Some(trim_http_quoted_string(value))
    })
}

fn trim_http_quoted_string(value: &str) -> String {
    value.trim().trim_matches('"').replace("\\\"", "\"")
}

fn find_all_subslices(haystack: &[u8], needle: &[u8]) -> Vec<usize> {
    let mut positions = Vec::new();
    let mut offset = 0;
    while let Some(position) = find_subslice(&haystack[offset..], needle) {
        let absolute = offset + position;
        positions.push(absolute);
        offset = absolute + needle.len();
    }
    positions
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

pub(super) async fn read_http_request(socket: &mut TcpStream) -> io::Result<Vec<u8>> {
    let mut buffer = vec![0_u8; REQUEST_READ_CHUNK_BYTES];
    let mut request = Vec::new();
    let mut expected_total_bytes = None;

    loop {
        let read = socket.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        request.extend_from_slice(&buffer[..read]);

        if expected_total_bytes.is_none()
            && let Some(headers_end) = request.windows(4).position(|slice| slice == b"\r\n\r\n")
        {
            let headers_end = headers_end + 4;
            let header_text = String::from_utf8_lossy(&request[..headers_end]);
            let content_length = extract_header(&header_text, "Content-Length")
                .and_then(|raw| raw.parse::<usize>().ok())
                .unwrap_or(0);
            expected_total_bytes = Some(headers_end.saturating_add(content_length));
        }

        if request.len() >= MAX_REQUEST_BYTES {
            break;
        }

        if let Some(expected_total_bytes) = expected_total_bytes
            && request.len() >= expected_total_bytes
        {
            break;
        }
    }

    Ok(request)
}

pub(super) async fn peek_http_request_line(socket: &mut TcpStream) -> io::Result<Option<String>> {
    let mut buffer = vec![0_u8; REQUEST_READ_CHUNK_BYTES];
    let read = socket.peek(&mut buffer).await?;
    if read == 0 {
        return Ok(None);
    }
    let request = String::from_utf8_lossy(&buffer[..read]).into_owned();
    Ok(request.lines().next().map(ToString::to_string))
}

pub(super) fn parse_request_line(request: &str) -> Option<(&str, &str)> {
    let line = request.lines().next()?;
    let mut parts = line.split_whitespace();
    let method = parts.next()?;
    let path = parts.next()?;
    Some((method, path))
}

pub(super) async fn write_sse_headers(socket: &mut TcpStream) -> io::Result<()> {
    socket
        .write_all(
            b"HTTP/1.1 200 OK\r\n\
Content-Type: text/event-stream; charset=utf-8\r\n\
Access-Control-Allow-Origin: *\r\n\
Cache-Control: no-cache, no-store, must-revalidate\r\n\
Connection: keep-alive\r\n\
X-Accel-Buffering: no\r\n\
\r\n",
        )
        .await
}

pub(super) async fn write_sse_event(socket: &mut TcpStream, payload: &str) -> io::Result<()> {
    socket
        .write_all(format!("data: {payload}\n\n").as_bytes())
        .await
}

pub(super) async fn write_sse_comment(socket: &mut TcpStream, comment: &str) -> io::Result<()> {
    socket
        .write_all(format!(": {comment}\n\n").as_bytes())
        .await
}

pub(super) fn build_response(
    status: &str,
    content_type: &str,
    body: &[u8],
    head_only: bool,
) -> Vec<u8> {
    let response_body = if head_only { &[][..] } else { body };
    let headers = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nAccess-Control-Allow-Origin: *\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let mut response = headers.into_bytes();
    response.extend_from_slice(response_body);
    response
}

pub(super) fn build_redirect_response(status: &str, location: &str, head_only: bool) -> Vec<u8> {
    let response_body = if head_only { &[][..] } else { b"redirecting" };
    let headers = format!(
        "HTTP/1.1 {status}\r\nLocation: {location}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nAccess-Control-Allow-Origin: *\r\nConnection: close\r\n\r\n",
        response_body.len()
    );
    let mut response = headers.into_bytes();
    response.extend_from_slice(response_body);
    response
}

pub(super) fn parse_named_header<'a>(line: &'a str, name: &str) -> Option<&'a str> {
    let (header_name, value) = line.split_once(':')?;
    if header_name.trim().eq_ignore_ascii_case(name) {
        Some(value.trim())
    } else {
        None
    }
}
