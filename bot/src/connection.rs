// connection.rs — низкоуровневый TCP/TLS коннектор к cTrader Open API.
//
// Протокол cTrader:
// ┌──────────────┬─────────────────────┐
// │  4 байта     │  N байт             │
// │  длина тела  │  Protobuf сообщение │
// └──────────────┴─────────────────────┘

use crate::Config;
use anyhow::{Context, Result};
use bytes::{Buf, BufMut, BytesMut};
use prost::Message;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_rustls::{client::TlsStream, rustls, TlsConnector};
use tracing::{debug, trace};

pub struct CTraderConnection {
    stream: TlsStream<TcpStream>,
    read_buf: BytesMut,
}

impl CTraderConnection {
    /// Устанавливает TLS соединение с cTrader API
    pub async fn new(config: &Config) -> Result<Self> {
        // --- Настройка TLS ---
        // Используем системные корневые сертификаты
        let mut root_store = rustls::RootCertStore::empty();
        for cert in rustls_native_certs::load_native_certs()? {
            root_store.add(cert)?;
        }

        let tls_config = rustls::ClientConfig::builder()
            .with_root_certificates(root_store)
            .with_no_client_auth();

        let connector = TlsConnector::from(Arc::new(tls_config));

        // --- TCP соединение ---
        let addr = format!("{}:{}", config.host, config.port);
        debug!("Подключаемся к {}", addr);

        let tcp_stream = TcpStream::connect(&addr)
            .await
            .context(format!("Не удалось подключиться к {}", addr))?;

        // Отключаем алгоритм Nagle — важно для latency!
        // Без этого мелкие пакеты будут буфферизироваться ~200ms
        tcp_stream.set_nodelay(true)?;

        // --- Оборачиваем TCP в TLS ---
        let server_name = rustls::pki_types::ServerName::try_from(config.host.clone())?;
        let tls_stream = connector.connect(server_name, tcp_stream).await?;

        debug!("TLS handshake завершён");

        Ok(Self {
            stream: tls_stream,
            read_buf: BytesMut::with_capacity(4096),
        })
    }

    /// Отправляет Protobuf сообщение.
    /// Формат: [4 байта длины][тело сообщения]
    pub async fn send<M: Message>(&mut self, message: &M) -> Result<()> {
        let body = message.encode_to_vec();
        let len = body.len() as u32;

        trace!("Отправка {} байт", len);

        let mut frame = BytesMut::with_capacity(4 + body.len());
        frame.put_u32(len);       // big-endian длина
        frame.put_slice(&body);   // тело

        self.stream.write_all(&frame).await?;
        Ok(())
    }

    /// Читает следующее Protobuf сообщение из потока.
    /// Блокирует до получения полного сообщения.
    pub async fn recv_raw(&mut self) -> Result<Vec<u8>> {
        // Читаем 4 байта длины
        loop {
            if self.read_buf.len() >= 4 {
                break;
            }
            self.fill_buf().await?;
        }

        let msg_len = (&self.read_buf[..4]).get_u32() as usize;
        self.read_buf.advance(4);

        trace!("Ожидаем сообщение {} байт", msg_len);

        // Читаем тело сообщения
        loop {
            if self.read_buf.len() >= msg_len {
                break;
            }
            self.fill_buf().await?;
        }

        let body = self.read_buf[..msg_len].to_vec();
        self.read_buf.advance(msg_len);

        Ok(body)
    }

    /// Читает байты из TLS потока в буфер
    async fn fill_buf(&mut self) -> Result<()> {
        let mut tmp = [0u8; 4096];
        let n = self.stream.read(&mut tmp).await?;
        if n == 0 {
            anyhow::bail!("Соединение закрыто сервером");
        }
        self.read_buf.put_slice(&tmp[..n]);
        Ok(())
    }
}
