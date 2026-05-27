//! Frame assembly: turn a [`MatchResult`] into the exact signed byte payload the on-chain engine
//! parses. The bytes produced here are what the committee signs and what the relay puts in the
//! `SubmitBatch` instruction data (after the 1-byte discriminator).

use {
    crate::mxe::MatchResult,
    bytemuck::bytes_of,
    protocol::{frame_len, BatchHeader, UserFill, HEADER_LEN},
};

/// Build the canonical frame bytes (`BatchHeader ++ [UserFill]`) for a matched batch.
///
/// `fill_count` is taken from the match result; the engine guarantees it is `≤ MAX_FILLS` (see
/// [`MxeCluster::match_batch`](crate::mxe::MxeCluster::match_batch)), so the `as u16` cast is exact.
pub fn build_frame(market: [u8; 32], epoch: u64, batch_id: u64, m: &MatchResult) -> Vec<u8> {
    let header = BatchHeader {
        market,
        epoch,
        batch_id,
        p2p_volume: m.p2p_volume,
        net_imbalance: m.net_imbalance,
        fill_count: m.fills.len() as u16,
        direction: m.direction,
        _pad: [0; 5],
    };

    let mut frame = Vec::with_capacity(frame_len(header.fill_count));
    frame.extend_from_slice(bytes_of(&header));
    for fill in &m.fills {
        frame.extend_from_slice(bytes_of::<UserFill>(fill));
    }
    debug_assert_eq!(frame.len(), HEADER_LEN + m.fills.len() * core::mem::size_of::<UserFill>());
    frame
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::{
            mxe::MxeCluster,
            order::{Order, Side},
        },
        protocol::{read_fill, read_header, validate_frame_len, DIRECTION_YES_HEAVY},
    };

    fn order(user: u8, side: Side, qty: u64) -> Order {
        Order {
            user: [user; 32],
            market: [1u8; 32],
            side,
            qty,
            limit_price: 0,
        }
    }

    #[test]
    fn frame_parses_back_through_protocol_readers() {
        let mut mxe = MxeCluster::new(4, 5);
        let m = mxe
            .match_batch(&[order(1, Side::Yes, 300), order(2, Side::No, 100)])
            .unwrap();
        let market = [7u8; 32];
        let frame = build_frame(market, 1, 1, &m);

        // Exactly the checks the on-chain engine runs first.
        let header = validate_frame_len(&frame).expect("frame must be exact length");
        assert_eq!(header.market, market);
        assert_eq!(header.epoch, 1);
        assert_eq!(header.direction, DIRECTION_YES_HEAVY);
        assert_eq!(header.net_imbalance, 200);
        assert_eq!(header.fill_count as usize, m.fills.len());
        assert_eq!(read_header(&frame).unwrap(), header);

        // Per-fill bytes round-trip identically.
        for (i, expected) in m.fills.iter().enumerate() {
            assert_eq!(&read_fill(&frame, i).unwrap(), expected);
        }
    }

    /// Re-derive the on-chain economic invariant from the frame: residual is strictly one-sided and
    /// equals the header's `net_imbalance`. A frame the engine would accept.
    #[test]
    fn frame_satisfies_on_chain_economic_check() {
        let mut mxe = MxeCluster::new(3, 11);
        let m = mxe
            .match_batch(&[
                order(1, Side::Yes, 333),
                order(2, Side::Yes, 333),
                order(3, Side::Yes, 334),
                order(9, Side::No, 500),
            ])
            .unwrap();
        let frame = build_frame([2u8; 32], 9, 9, &m);
        let header = validate_frame_len(&frame).unwrap();

        let mut sum_res_yes = 0u64;
        let mut sum_res_no = 0u64;
        for i in 0..header.fill_count as usize {
            let f = read_fill(&frame, i).unwrap();
            sum_res_yes += f.residual_yes;
            sum_res_no += f.residual_no;
        }
        let (heavy, light) = match header.direction {
            DIRECTION_YES_HEAVY => (sum_res_yes, sum_res_no),
            _ => (sum_res_no, sum_res_yes),
        };
        assert_eq!(light, 0, "residual must be one-sided");
        assert_eq!(heavy, header.net_imbalance, "Σ residual must equal net imbalance");
    }
}
