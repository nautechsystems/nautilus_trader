//! OKX-specific subscription helpers.

use ustr::Ustr;

use crate::{
    common::enums::OKXInstrumentType,
    websocket::{
        enums::OKXWsChannel,
        messages::{OKXSubscriptionArg, OKXWebSocketArg},
    },
};

fn topic_from_parts(
    channel: &OKXWsChannel,
    inst_id: Option<&Ustr>,
    inst_family: Option<&Ustr>,
    inst_type: Option<&OKXInstrumentType>,
    bar: Option<&Ustr>,
) -> String {
    let base = channel.as_ref();

    if let Some(inst_id) = inst_id {
        let inst_id = inst_id.as_str();

        if let Some(bar) = bar {
            format!("{base}:{inst_id}:{}", bar.as_str())
        } else {
            format!("{base}:{inst_id}")
        }
    } else if let Some(inst_family) = inst_family {
        format!("{base}:{}", inst_family.as_str())
    } else if let Some(inst_type) = inst_type {
        format!("{base}:{}", inst_type.as_ref())
    } else {
        base.to_string()
    }
}

pub(crate) fn topic_from_subscription_arg(arg: &OKXSubscriptionArg) -> String {
    topic_from_parts(
        &arg.channel,
        arg.inst_id.as_ref(),
        arg.inst_family.as_ref(),
        arg.inst_type.as_ref(),
        None,
    )
}

pub(crate) fn topic_from_websocket_arg(arg: &OKXWebSocketArg) -> String {
    topic_from_parts(
        &arg.channel,
        arg.inst_id.as_ref(),
        arg.inst_family.as_ref(),
        arg.inst_type.as_ref(),
        arg.bar.as_ref(),
    )
}
