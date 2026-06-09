use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

pub struct QmpClient {
    stream: UnixStream,
    reader: BufReader<UnixStream>,
}

impl QmpClient {
    pub fn connect(socket_path: &str) -> Result<Self, String> {
        let stream = UnixStream::connect(socket_path)
            .map_err(|e| format!("QMP connect failed: {e}"))?;
        stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
        stream.set_write_timeout(Some(Duration::from_secs(5))).ok();
        let reader_stream = stream.try_clone()
            .map_err(|e| format!("QMP clone failed: {e}"))?;
        let mut client = Self {
            stream,
            reader: BufReader::new(reader_stream),
        };
        client.read_response()?;
        client.send_raw(r#"{"execute":"qmp_capabilities"}"#)?;
        client.read_response()?;
        Ok(client)
    }

    pub fn screendump(&mut self, path: &str) -> Result<(), String> {
        let cmd = format!(
            r#"{{"execute":"screendump","arguments":{{"filename":"{path}","format":"png"}}}}"#
        );
        self.send_raw(&cmd)?;
        self.read_response()?;
        Ok(())
    }

    pub fn send_command(&mut self, execute: &str, args: Option<&str>) -> Result<String, String> {
        let cmd = if let Some(a) = args {
            format!(r#"{{"execute":"{execute}","arguments":{a}}}"#)
        } else {
            format!(r#"{{"execute":"{execute}"}}"#)
        };
        self.send_raw(&cmd)?;
        self.read_response()
    }

    fn send_raw(&mut self, msg: &str) -> Result<(), String> {
        self.stream.write_all(msg.as_bytes())
            .map_err(|e| format!("QMP write failed: {e}"))?;
        self.stream.write_all(b"\n")
            .map_err(|e| format!("QMP write newline failed: {e}"))?;
        Ok(())
    }

    fn read_response(&mut self) -> Result<String, String> {
        let mut line = String::new();
        self.reader.read_line(&mut line)
            .map_err(|e| format!("QMP read failed: {e}"))?;
        Ok(line.trim().to_string())
    }
}
