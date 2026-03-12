use common::types::{Address, Dex};

use crate::rpc::RawTransaction;

/// Known Uniswap V2 Router function selectors.
const SWAP_EXACT_TOKENS_FOR_TOKENS: [u8; 4] = [0x38, 0xed, 0x17, 0x38];
const SWAP_TOKENS_FOR_EXACT_TOKENS: [u8; 4] = [0x88, 0x03, 0xdb, 0xee];

/// Known Uniswap V3 Router function selectors.
const EXACT_INPUT_SINGLE: [u8; 4] = [0x41, 0x4b, 0xf3, 0x89];

/// A parsed DEX swap extracted from a pending transaction.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedSwap {
    pub dex: Dex,
    pub token_in: Address,
    pub token_out: Address,
    pub amount: u128,
    pub sender: Address,
}

/// Identifies the type of swap detected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwapKind {
    UniV2SwapExactTokensForTokens,
    UniV2SwapTokensForExactTokens,
    UniV3ExactInputSingle,
}

/// Transaction parser that detects DEX swaps from raw calldata.
pub struct TxParser;

impl TxParser {
    /// Try to detect a DEX swap in the transaction calldata.
    /// Returns `None` if the calldata doesn't match any known swap selector.
    pub fn parse_swap(tx: &RawTransaction) -> Option<ParsedSwap> {
        let selector = Self::extract_selector(&tx.input)?;
        let kind = Self::identify_swap(selector)?;

        match kind {
            SwapKind::UniV2SwapExactTokensForTokens => Self::parse_uni_v2_swap(tx, kind),
            SwapKind::UniV2SwapTokensForExactTokens => Self::parse_uni_v2_swap(tx, kind),
            SwapKind::UniV3ExactInputSingle => Self::parse_uni_v3_exact_input_single(tx),
        }
    }

    /// Detect if the calldata matches a known swap selector (without full parsing).
    pub fn is_swap(input: &[u8]) -> Option<SwapKind> {
        let selector = Self::extract_selector(input)?;
        Self::identify_swap(selector)
    }

    fn extract_selector(input: &[u8]) -> Option<[u8; 4]> {
        if input.len() < 4 {
            return None;
        }
        let mut sel = [0u8; 4];
        sel.copy_from_slice(&input[..4]);
        Some(sel)
    }

    fn identify_swap(selector: [u8; 4]) -> Option<SwapKind> {
        match selector {
            SWAP_EXACT_TOKENS_FOR_TOKENS => Some(SwapKind::UniV2SwapExactTokensForTokens),
            SWAP_TOKENS_FOR_EXACT_TOKENS => Some(SwapKind::UniV2SwapTokensForExactTokens),
            EXACT_INPUT_SINGLE => Some(SwapKind::UniV3ExactInputSingle),
            _ => None,
        }
    }

    /// Parse Uniswap V2 swap calldata.
    ///
    /// Both `swapExactTokensForTokens` and `swapTokensForExactTokens` share the same
    /// ABI layout:
    ///   - bytes 4..36:   amountIn (or amountOut)
    ///   - bytes 36..68:  amountOutMin (or amountInMax)
    ///   - bytes 68..100: offset to path array
    ///   - bytes 100..132: to address
    ///   - bytes 132..164: deadline
    ///   - bytes 164..196: path length
    ///   - bytes 196..228: path[0] (token_in) — address is in last 20 bytes of 32-byte word
    ///   - bytes 228..260: path[1] (token_out) if path length >= 2
    fn parse_uni_v2_swap(tx: &RawTransaction, _kind: SwapKind) -> Option<ParsedSwap> {
        let input = &tx.input;
        // Minimum: 4 (selector) + 5*32 (params) + 32 (path len) + 2*32 (at least 2 path elements) = 260
        if input.len() < 260 {
            return None;
        }

        // Amount is in the first parameter word (bytes 4..36).
        let amount = u128_from_be_bytes(&input[4..36]);

        // Path length at offset 164..196
        let path_len = u128_from_be_bytes(&input[164..196]) as usize;
        if path_len < 2 {
            return None;
        }

        // token_in = path[0], last 20 bytes of word at 196..228
        let token_in = address_from_word(&input[196..228]);
        // token_out = last element: path[path_len-1]
        let last_token_offset = 196 + (path_len - 1) * 32;
        if input.len() < last_token_offset + 32 {
            return None;
        }
        let token_out = address_from_word(&input[last_token_offset..last_token_offset + 32]);

        Some(ParsedSwap {
            dex: Dex::UniswapV2,
            token_in,
            token_out,
            amount,
            sender: Address(tx.from),
        })
    }

    /// Parse Uniswap V3 `exactInputSingle` calldata.
    ///
    /// ExactInputSingleParams is a struct encoded as:
    ///   - bytes 4..36:   tokenIn (address, last 20 bytes)
    ///   - bytes 36..68:  tokenOut (address, last 20 bytes)
    ///   - bytes 68..100: fee (uint24)
    ///   - bytes 100..132: recipient (address)
    ///   - bytes 132..164: deadline (uint256)
    ///   - bytes 164..196: amountIn (uint256)
    ///   - bytes 196..228: amountOutMinimum (uint256)
    ///   - bytes 228..260: sqrtPriceLimitX96 (uint160)
    fn parse_uni_v3_exact_input_single(tx: &RawTransaction) -> Option<ParsedSwap> {
        let input = &tx.input;
        // Need at least 4 + 8*32 = 260 bytes
        if input.len() < 260 {
            return None;
        }

        let token_in = address_from_word(&input[4..36]);
        let token_out = address_from_word(&input[36..68]);
        let amount = u128_from_be_bytes(&input[164..196]);

        Some(ParsedSwap {
            dex: Dex::UniswapV3,
            token_in,
            token_out,
            amount,
            sender: Address(tx.from),
        })
    }
}

/// Extract an Address from the last 20 bytes of a 32-byte ABI word.
fn address_from_word(word: &[u8]) -> Address {
    let mut addr = [0u8; 20];
    addr.copy_from_slice(&word[12..32]);
    Address(addr)
}

/// Extract a u128 from the last 16 bytes of a 32-byte ABI word.
/// (Sufficient for most token amounts; amounts > u128::MAX are astronomically unlikely.)
fn u128_from_be_bytes(word: &[u8]) -> u128 {
    // Take last 16 bytes of the 32-byte word
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&word[16..32]);
    u128::from_be_bytes(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_sender() -> [u8; 20] {
        [0xAB; 20]
    }

    fn make_tx(input: Vec<u8>) -> RawTransaction {
        RawTransaction {
            hash: common::types::TxHash([0u8; 32]),
            from: make_sender(),
            to: Some([0xCC; 20]),
            value: 0,
            input,
            gas_price: 20_000_000_000,
        }
    }

    /// Build a minimal swapExactTokensForTokens calldata.
    fn build_uni_v2_calldata(selector: [u8; 4], amount: u128, token_in: [u8; 20], token_out: [u8; 20]) -> Vec<u8> {
        let mut data = Vec::with_capacity(292);

        // Selector (4 bytes)
        data.extend_from_slice(&selector);

        // amountIn (32 bytes) — first param
        let mut word = [0u8; 32];
        word[16..32].copy_from_slice(&amount.to_be_bytes());
        data.extend_from_slice(&word);

        // amountOutMin (32 bytes) — second param
        data.extend_from_slice(&[0u8; 32]);

        // offset to path (32 bytes) — points to byte 160 from start of params = 0xa0
        let mut offset_word = [0u8; 32];
        offset_word[31] = 0xa0;
        data.extend_from_slice(&offset_word);

        // to address (32 bytes) — fourth param
        data.extend_from_slice(&[0u8; 32]);

        // deadline (32 bytes) — fifth param
        data.extend_from_slice(&[0u8; 32]);

        // path length = 2 (32 bytes)
        let mut len_word = [0u8; 32];
        len_word[31] = 2;
        data.extend_from_slice(&len_word);

        // path[0] = token_in (32 bytes, address in last 20)
        let mut in_word = [0u8; 32];
        in_word[12..32].copy_from_slice(&token_in);
        data.extend_from_slice(&in_word);

        // path[1] = token_out (32 bytes, address in last 20)
        let mut out_word = [0u8; 32];
        out_word[12..32].copy_from_slice(&token_out);
        data.extend_from_slice(&out_word);

        data
    }

    /// Build a minimal Uniswap V3 exactInputSingle calldata.
    fn build_uni_v3_calldata(token_in: [u8; 20], token_out: [u8; 20], amount: u128) -> Vec<u8> {
        let mut data = Vec::with_capacity(260);

        // Selector
        data.extend_from_slice(&EXACT_INPUT_SINGLE);

        // tokenIn (32 bytes)
        let mut in_word = [0u8; 32];
        in_word[12..32].copy_from_slice(&token_in);
        data.extend_from_slice(&in_word);

        // tokenOut (32 bytes)
        let mut out_word = [0u8; 32];
        out_word[12..32].copy_from_slice(&token_out);
        data.extend_from_slice(&out_word);

        // fee (32 bytes)
        let mut fee_word = [0u8; 32];
        fee_word[31] = 0x0b; // 3000 = 0x0bb8, but doesn't matter for test
        fee_word[30] = 0xb8;
        data.extend_from_slice(&fee_word);

        // recipient (32 bytes)
        data.extend_from_slice(&[0u8; 32]);

        // deadline (32 bytes)
        data.extend_from_slice(&[0u8; 32]);

        // amountIn (32 bytes)
        let mut amount_word = [0u8; 32];
        amount_word[16..32].copy_from_slice(&amount.to_be_bytes());
        data.extend_from_slice(&amount_word);

        // amountOutMinimum (32 bytes)
        data.extend_from_slice(&[0u8; 32]);

        // sqrtPriceLimitX96 (32 bytes)
        data.extend_from_slice(&[0u8; 32]);

        data
    }

    #[test]
    fn test_parse_uni_v2_swap_exact_tokens_for_tokens() {
        let token_in = [0x11; 20];
        let token_out = [0x22; 20];
        let amount: u128 = 1_000_000_000_000_000_000; // 1 ETH in wei

        let calldata = build_uni_v2_calldata(SWAP_EXACT_TOKENS_FOR_TOKENS, amount, token_in, token_out);
        let tx = make_tx(calldata);

        let parsed = TxParser::parse_swap(&tx).expect("should parse V2 swap");
        assert_eq!(parsed.dex, Dex::UniswapV2);
        assert_eq!(parsed.token_in, Address(token_in));
        assert_eq!(parsed.token_out, Address(token_out));
        assert_eq!(parsed.amount, amount);
        assert_eq!(parsed.sender, Address(make_sender()));
    }

    #[test]
    fn test_parse_uni_v2_swap_tokens_for_exact_tokens() {
        let token_in = [0x33; 20];
        let token_out = [0x44; 20];
        let amount: u128 = 500_000_000;

        let calldata = build_uni_v2_calldata(SWAP_TOKENS_FOR_EXACT_TOKENS, amount, token_in, token_out);
        let tx = make_tx(calldata);

        let parsed = TxParser::parse_swap(&tx).expect("should parse V2 swap");
        assert_eq!(parsed.dex, Dex::UniswapV2);
        assert_eq!(parsed.token_in, Address(token_in));
        assert_eq!(parsed.token_out, Address(token_out));
        assert_eq!(parsed.amount, amount);
    }

    #[test]
    fn test_parse_uni_v3_exact_input_single() {
        let token_in = [0xAA; 20];
        let token_out = [0xBB; 20];
        let amount: u128 = 2_000_000_000_000_000_000;

        let calldata = build_uni_v3_calldata(token_in, token_out, amount);
        let tx = make_tx(calldata);

        let parsed = TxParser::parse_swap(&tx).expect("should parse V3 swap");
        assert_eq!(parsed.dex, Dex::UniswapV3);
        assert_eq!(parsed.token_in, Address(token_in));
        assert_eq!(parsed.token_out, Address(token_out));
        assert_eq!(parsed.amount, amount);
    }

    #[test]
    fn test_unknown_selector_returns_none() {
        let tx = make_tx(vec![0xFF, 0xFF, 0xFF, 0xFF, 0x00]);
        assert!(TxParser::parse_swap(&tx).is_none());
    }

    #[test]
    fn test_short_calldata_returns_none() {
        let tx = make_tx(vec![0x38, 0xed, 0x17]);
        assert!(TxParser::parse_swap(&tx).is_none());
    }

    #[test]
    fn test_is_swap_detects_selectors() {
        assert_eq!(
            TxParser::is_swap(&[0x38, 0xed, 0x17, 0x38]),
            Some(SwapKind::UniV2SwapExactTokensForTokens)
        );
        assert_eq!(
            TxParser::is_swap(&[0x88, 0x03, 0xdb, 0xee]),
            Some(SwapKind::UniV2SwapTokensForExactTokens)
        );
        assert_eq!(
            TxParser::is_swap(&[0x41, 0x4b, 0xf3, 0x89]),
            Some(SwapKind::UniV3ExactInputSingle)
        );
        assert_eq!(TxParser::is_swap(&[0x00, 0x00, 0x00, 0x00]), None);
    }

    #[test]
    fn test_empty_input_returns_none() {
        let tx = make_tx(vec![]);
        assert!(TxParser::parse_swap(&tx).is_none());
    }
}
