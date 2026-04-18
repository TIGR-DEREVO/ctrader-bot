// auth.rs — двухэтапная авторизация в cTrader Open API.
//
// Все сообщения оборачиваются в ProtoMessage (envelope):
//   ProtoMessage { payload_type, payload: Some(inner.encode_to_vec()), client_msg_id }
//
// Шаг 1: ProtoOAApplicationAuthReq → ProtoOAApplicationAuthRes
// Шаг 2: ProtoOAAccountAuthReq     → ProtoOAAccountAuthRes

use crate::connection::CTraderConnection;
use crate::Config;
use anyhow::{bail, Result};
use prost::Message;
use tracing::{debug, info};

mod proto {
    include!(concat!(env!("OUT_DIR"), "/_.rs"));
}

/// Оборачивает inner-сообщение в ProtoMessage envelope
fn wrap(payload_type: u32, inner: &impl Message) -> proto::ProtoMessage {
    proto::ProtoMessage {
        payload_type,
        payload: Some(inner.encode_to_vec()),
        client_msg_id: None,
    }
}

/// Читает ProtoMessage из соединения и проверяет на ошибки
async fn recv_msg(conn: &mut CTraderConnection) -> Result<proto::ProtoMessage> {
    let raw = conn.recv_raw().await?;
    let msg = proto::ProtoMessage::decode(raw.as_slice())?;

    // Проверяем, не вернул ли сервер ошибку
    if msg.payload_type == proto::ProtoPayloadType::ErrorRes as u32 {
        if let Some(ref payload) = msg.payload {
            let err = proto::ProtoErrorRes::decode(payload.as_slice())?;
            bail!(
                "Сервер вернул ошибку: {:?} — {}",
                err.error_code,
                err.description.unwrap_or_default()
            );
        }
        bail!("Сервер вернул ErrorRes без payload");
    }

    // Проверяем OA-специфичные ошибки (payload_type == 2142)
    if msg.payload_type == proto::ProtoOaPayloadType::ProtoOaErrorRes as u32 {
        if let Some(ref payload) = msg.payload {
            let err = proto::ProtoOaErrorRes::decode(payload.as_slice())?;
            bail!(
                "OA ошибка: {:?} — {}",
                err.error_code,
                err.description.unwrap_or_default()
            );
        }
        bail!("Сервер вернул ProtoOAErrorRes без payload");
    }

    Ok(msg)
}

pub async fn authenticate(conn: &mut CTraderConnection, config: &Config) -> Result<()> {
    // ── Шаг 1: Авторизация приложения ──
    debug!("Шаг 1: авторизация приложения...");

    let app_auth_req = proto::ProtoOaApplicationAuthReq {
        payload_type: Some(proto::ProtoOaPayloadType::ProtoOaApplicationAuthReq as i32),
        client_id: config.client_id.clone(),
        client_secret: config.client_secret.clone(),
    };

    let envelope = wrap(
        proto::ProtoOaPayloadType::ProtoOaApplicationAuthReq as u32,
        &app_auth_req,
    );
    conn.send(&envelope).await?;

    let resp = recv_msg(conn).await?;
    if resp.payload_type != proto::ProtoOaPayloadType::ProtoOaApplicationAuthRes as u32 {
        bail!(
            "Ожидали ApplicationAuthRes (2101), получили payload_type={}",
            resp.payload_type
        );
    }
    debug!("Приложение авторизовано");

    // ── Шаг 2: Авторизация торгового аккаунта ──
    debug!("Шаг 2: авторизация аккаунта {}...", config.account_id);

    let account_auth_req = proto::ProtoOaAccountAuthReq {
        payload_type: Some(proto::ProtoOaPayloadType::ProtoOaAccountAuthReq as i32),
        ctid_trader_account_id: config.account_id,
        access_token: config.access_token.clone(),
    };

    let envelope = wrap(
        proto::ProtoOaPayloadType::ProtoOaAccountAuthReq as u32,
        &account_auth_req,
    );
    conn.send(&envelope).await?;

    let resp = recv_msg(conn).await?;
    if resp.payload_type != proto::ProtoOaPayloadType::ProtoOaAccountAuthRes as u32 {
        bail!(
            "Ожидали AccountAuthRes (2103), получили payload_type={}",
            resp.payload_type
        );
    }

    info!("Аккаунт {} авторизован успешно", config.account_id);
    Ok(())
}
