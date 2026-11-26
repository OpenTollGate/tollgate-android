//! The v2 wire contract the Android core speaks, pinned by tests.
//!
//! The core was written against the request/response protocol (a `PriceSheet`
//! carried a price, a `MeteringReport` carried usage, a `BootstrapToken` paid).
//! Protocol v2 has none of those: a buyer opens a session, funds a channel, and
//! buys windows with signed `TopUp`s. `Offer` deliberately carries **no price**
//! — "delivery is one voucher per unit" — so a price shown in the UI has to come
//! from the mint's denomination, not from the gateway's advert. That is the
//! single most consequential difference for the app, and the reason `Detected`
//! no longer has a `price` field.
//!
//! These tests are host-only: they exercise the real `tollgate-protocol` codec
//! and framing, no Android, no sockets.

#[cfg(test)]
mod tests {
    use tollgate_protocol::{
        encode_frame, Accept, Announce, ChannelReady, ChannelUpdate, Message, MsgType, Offer,
        PubKey, ReasonCode, Reject, TopUp, PROTOCOL_VERSION,
    };

    use crate::v2::{self, OfferView, RejectReason, WireError};

    fn gateway_key() -> PubKey {
        // Any 33-byte compressed key: the codec does not verify, and these
        // tests are about shape, not cryptography.
        let mut bytes = [0u8; 33];
        bytes[0] = 0x02;
        bytes[32] = 0x11;
        PubKey(bytes)
    }

    fn offer_message() -> Message {
        Message::Offer(Offer {
            accepted_mints: vec!["https://mint.example".into()],
            unit: "byte".into(),
            min_window_ms: 60_000,
            max_window_ms: 86_400_000,
            from_payer_weight: 1,
            no_charge: false,
            min_reserved_rate: 0,
            min_topup_gap_ms: 5_000,
        })
    }

    fn frames(messages: &[Message]) -> Vec<u8> {
        let mut out = Vec::new();
        for message in messages {
            encode_frame(message, &mut out).expect("encode");
        }
        out
    }

    #[test]
    fn a_gateway_announce_reports_its_unit_and_version() {
        let bytes = frames(&[Message::Announce(Announce {
            version: PROTOCOL_VERSION,
            pubkey: gateway_key(),
            unit: "byte".into(),
            capabilities: 0,
        })]);
        let seen = v2::read_messages(&bytes).expect("two whole frames are not an error");
        assert_eq!(seen.len(), 1);
        let detected = v2::detect_from(&seen).expect("an Announce is present");
        assert_eq!(detected.unit, "byte");
        assert_eq!(detected.version, PROTOCOL_VERSION);
        assert_eq!(detected.pubkey_hex, hex::encode(gateway_key().as_bytes()));
        // No price: v2's Offer carries terms, not money. A caller that wants a
        // price must ask the mint, and the type says so by having no field.
        assert_eq!(detected.offer, None, "no Offer in this answer");
    }

    #[test]
    fn the_offer_terms_are_carried_through_unchanged() {
        let bytes = frames(&[
            Message::Announce(Announce {
                version: PROTOCOL_VERSION,
                pubkey: gateway_key(),
                unit: "byte".into(),
                capabilities: 0,
            }),
            offer_message(),
        ]);
        let seen = v2::read_messages(&bytes).expect("decodes");
        let detected = v2::detect_from(&seen).expect("an Announce is present");
        assert_eq!(
            detected.offer,
            Some(OfferView {
                accepted_mints: vec!["https://mint.example".into()],
                unit: "byte".into(),
                min_window_ms: 60_000,
                max_window_ms: 86_400_000,
                from_payer_weight: 1,
                no_charge: false,
                min_reserved_rate: 0,
                min_topup_gap_ms: 5_000,
            })
        );
    }

    #[test]
    fn several_frames_in_one_read_are_all_seen() {
        // A single TCP read can carry more than one message; dropping the tail
        // would lose the Offer that arrives right behind the Announce.
        let bytes = frames(&[
            Message::Announce(Announce {
                version: PROTOCOL_VERSION,
                pubkey: gateway_key(),
                unit: "byte".into(),
                capabilities: 0,
            }),
            offer_message(),
            Message::Accept(Accept {
                funding: vec![1, 2, 3],
            }),
        ]);
        let seen = v2::read_messages(&bytes).expect("decodes");
        assert_eq!(seen.len(), 3, "every whole frame is returned");
    }

    #[test]
    fn a_half_arrived_frame_is_left_pending_not_reported_as_an_error() {
        let mut bytes = frames(&[offer_message(), offer_message()]);
        let cut = bytes.len() - 5;
        bytes.truncate(cut);
        let seen = v2::read_messages(&bytes).expect("a partial tail is not an error");
        assert_eq!(seen.len(), 1);
    }

    #[test]
    fn a_refusal_is_classified_by_its_reason_code() {
        for (code, reason) in [
            (ReasonCode::MintNotAccepted, RejectReason::MintNotAccepted),
            (ReasonCode::UnitNotAccepted, RejectReason::UnitNotAccepted),
            (ReasonCode::OutOfRange, RejectReason::OutOfRange),
            (ReasonCode::FundingInvalid, RejectReason::FundingInvalid),
            (ReasonCode::RateExceedsCapacity, RejectReason::RateExceedsCapacity),
            (ReasonCode::GrantExceedsChannel, RejectReason::GrantExceedsChannel),
            (ReasonCode::VersionUnsupported, RejectReason::VersionUnsupported),
            (ReasonCode::TooSoon, RejectReason::TooSoon),
            (ReasonCode::Other, RejectReason::Other),
        ] {
            let bytes = frames(&[Message::Reject(Reject {
                rejected_type: MsgType::TopUp as u8,
                reason: code,
                text: None,
            })]);
            let seen = v2::read_messages(&bytes).expect("decodes");
            assert_eq!(v2::refusal_in(&seen), Some(reason), "{code:?}");
        }
    }

    #[test]
    fn a_top_up_and_a_channel_ready_are_recognised_but_not_mistaken_for_a_refusal() {
        let bytes = frames(&[
            Message::ChannelReady(ChannelReady {
                channel_id: tollgate_protocol::ChannelId([7u8; 32]),
            }),
            // A TopUp must ratchet at least one channel: an empty update list
            // is refused at encode time (`NoChannelUpdates`), not silently sent.
            Message::TopUp(TopUp {
                updates: vec![ChannelUpdate {
                    channel_id: tollgate_protocol::ChannelId([7u8; 32]),
                    cumulative: 1000,
                    signature: tollgate_protocol::Signature([0u8; 64]),
                }],
                window_ms: 600_000,
                reserved_rate: 0,
            }),
        ]);
        let seen = v2::read_messages(&bytes).expect("decodes");
        assert_eq!(seen.len(), 2);
        assert_eq!(v2::refusal_in(&seen), None);
    }

    #[test]
    fn a_corrupt_frame_is_an_error_not_silently_dropped() {
        // The length prefix is intact; the body is not CBOR. The stream stays
        // in sync, so this must surface rather than look like an empty answer.
        let mut bytes = vec![4, 0];
        bytes.extend_from_slice(&[0xff, 0xff, 0xff, 0xff]);
        assert!(matches!(v2::read_messages(&bytes), Err(WireError::Codec(_))));
    }

    #[test]
    fn an_empty_read_is_an_empty_answer_not_an_error() {
        assert_eq!(v2::read_messages(&[]).expect("nothing is not a failure").len(), 0);
    }
}
