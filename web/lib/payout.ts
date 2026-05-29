import { MarketState, UserPosition } from "./state";
import { OUTCOME, MIDPOINT, SCALE } from "./constants";

const MID = BigInt(MIDPOINT);
const SC = BigInt(SCALE);

/**
 * Base-unit collateral the `ClaimWinnings` instruction pays for a resolved market — mirrors
 * `program/src/instructions/claim_winnings.rs` exactly:
 *   - YES won:  yes_qty × $1            = collateral_for(yes_qty, SCALE)
 *   - NO won:   no_qty  × $1            = collateral_for(no_qty,  SCALE)
 *   - INVALID:  (yes + no) each × $0.50 = collateral_for(yes, MID) + collateral_for(no, MID)
 *               (NOT the full deposited collateral — that is withdrawn separately)
 * Each leg is floored independently, matching the program's two `collateral_for` calls.
 */
export function claimPayout(p: UserPosition, m: MarketState): bigint {
  switch (m.outcome) {
    case OUTCOME.YES_WON:
      return p.yesQty;
    case OUTCOME.NO_WON:
      return p.noQty;
    case OUTCOME.INVALID:
      return (p.yesQty * MID) / SC + (p.noQty * MID) / SC;
    default:
      return 0n;
  }
}
