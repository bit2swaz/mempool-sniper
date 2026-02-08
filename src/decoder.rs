use alloy::primitives::{Address, U256};
use alloy::sol;
use alloy::sol_types::SolCall;
use anyhow::Result;

sol! {
    interface IUniswapV2Router {
        function swapExactETHForTokens(
            uint amountOutMin,
            address[] calldata path,
            address to,
            uint deadline
        ) external payable returns (uint[] memory amounts);

        function swapExactTokensForETH(
            uint amountIn,
            uint amountOutMin,
            address[] calldata path,
            address to,
            uint deadline
        ) external returns (uint[] memory amounts);

        function swapExactTokensForTokens(
            uint amountIn,
            uint amountOutMin,
            address[] calldata path,
            address to,
            uint deadline
        ) external returns (uint[] memory amounts);
    }
}

sol! {
    struct ExactInputSingleParams {
        address tokenIn;
        address tokenOut;
        uint24 fee;
        address recipient;
        uint256 deadline;
        uint256 amountIn;
        uint256 amountOutMinimum;
        uint160 sqrtPriceLimitX96;
    }

    struct ExactInputParams {
        bytes path;
        address recipient;
        uint256 deadline;
        uint256 amountIn;
        uint256 amountOutMinimum;
    }

    interface IUniswapV3Router {
        function exactInputSingle(ExactInputSingleParams calldata params) external payable returns (uint256 amountOut);
        function exactInput(ExactInputParams calldata params) external payable returns (uint256 amountOut);
        function multicall(uint256 deadline, bytes[] calldata data) external payable returns (bytes[] memory results);
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DecodedTx {
    pub amount_out_min: U256,
    pub path: Vec<Address>,
    pub to: Address,
    pub deadline: U256,
    pub effective_value: U256,
    pub method: String,
}

pub const SWAP_EXACT_ETH_FOR_TOKENS: [u8; 4] = [0x7f, 0xf3, 0x6a, 0xb5];

pub const SWAP_EXACT_TOKENS_FOR_ETH: [u8; 4] = [0x18, 0xcb, 0xaf, 0xe5];

pub const SWAP_EXACT_TOKENS_FOR_TOKENS: [u8; 4] = [0x38, 0xed, 0x17, 0x39];

pub const SWAP_ETH_FOR_EXACT_TOKENS: [u8; 4] = [0xfb, 0x3b, 0xdb, 0x41];

pub const EXACT_INPUT_SINGLE: [u8; 4] = [0x41, 0x4b, 0xf3, 0x89];

pub const EXACT_INPUT: [u8; 4] = [0xc0, 0x4b, 0x8d, 0x59];

pub const MULTICALL_V3: [u8; 4] = [0x5a, 0xe4, 0x01, 0xdc];

pub const EXECUTE_V3: [u8; 4] = [0x24, 0x85, 0x62, 0x29];

pub const AGGREGATOR_SWAP: [u8; 4] = [0x12, 0xaa, 0x3c, 0xaf];

pub const UNISWAP_V3_SWAP_TO: [u8; 4] = [0xbc, 0x65, 0x1e, 0x96];

pub fn is_target_transaction(_input_data: &[u8]) -> bool {
    true
}

pub fn decode_transaction(input_data: &[u8], tx_value: U256) -> Result<DecodedTx> {
    if input_data.is_empty() {
        return Ok(DecodedTx {
            amount_out_min: U256::ZERO,
            path: vec![],
            to: Address::ZERO,
            deadline: U256::ZERO,
            effective_value: tx_value,
            method: "Native Transfer".to_string(),
        });
    }

    if input_data.len() < 4 {
        return Ok(DecodedTx {
            amount_out_min: U256::ZERO,
            path: vec![],
            to: Address::ZERO,
            deadline: U256::ZERO,
            effective_value: tx_value,
            method: "Unknown".to_string(),
        });
    }

    let selector = &input_data[0..4];

    let (amount_out_min, path, to, deadline, amount_in, method) = match selector {
        s if s == SWAP_EXACT_ETH_FOR_TOKENS => {
            match IUniswapV2Router::swapExactETHForTokensCall::abi_decode(input_data, true) {
                Ok(call) => (
                    call.amountOutMin,
                    call.path,
                    call.to,
                    call.deadline,
                    U256::ZERO,
                    "swapExactETHForTokens",
                ),
                Err(_) => {
                    return Ok(DecodedTx {
                        amount_out_min: U256::ZERO,
                        path: vec![],
                        to: Address::ZERO,
                        deadline: U256::ZERO,
                        effective_value: tx_value,
                        method: "Unknown".to_string(),
                    })
                }
            }
        }

        s if s == SWAP_EXACT_TOKENS_FOR_ETH => {
            match IUniswapV2Router::swapExactTokensForETHCall::abi_decode(input_data, true) {
                Ok(call) => (
                    call.amountOutMin,
                    call.path,
                    call.to,
                    call.deadline,
                    call.amountIn,
                    "swapExactTokensForETH",
                ),
                Err(_) => {
                    return Ok(DecodedTx {
                        amount_out_min: U256::ZERO,
                        path: vec![],
                        to: Address::ZERO,
                        deadline: U256::ZERO,
                        effective_value: tx_value,
                        method: "Unknown".to_string(),
                    })
                }
            }
        }

        s if s == SWAP_EXACT_TOKENS_FOR_TOKENS => {
            match IUniswapV2Router::swapExactTokensForTokensCall::abi_decode(input_data, true) {
                Ok(call) => (
                    call.amountOutMin,
                    call.path,
                    call.to,
                    call.deadline,
                    call.amountIn, 
                    "swapExactTokensForTokens",
                ),
                Err(_) => {
                    return Ok(DecodedTx {
                        amount_out_min: U256::ZERO,
                        path: vec![],
                        to: Address::ZERO,
                        deadline: U256::ZERO,
                        effective_value: tx_value,
                        method: "Unknown".to_string(),
                    })
                }
            }
        }

        s if s == EXACT_INPUT_SINGLE => {
            match IUniswapV3Router::exactInputSingleCall::abi_decode(input_data, true) {
                Ok(call) => {
                    let params = call.params;
                    (
                        params.amountOutMinimum,
                        vec![params.tokenIn, params.tokenOut],
                        params.recipient,
                        params.deadline,
                        params.amountIn,
                        "exactInputSingle",
                    )
                }
                Err(_) => {
                    return Ok(DecodedTx {
                        amount_out_min: U256::ZERO,
                        path: vec![],
                        to: Address::ZERO,
                        deadline: U256::ZERO,
                        effective_value: tx_value,
                        method: "Unknown".to_string(),
                    })
                }
            }
        }

        s if s == EXACT_INPUT => {
            match IUniswapV3Router::exactInputCall::abi_decode(input_data, true) {
                Ok(call) => {
                    let params = call.params;
                    (
                        params.amountOutMinimum,
                        vec![],
                        params.recipient,
                        params.deadline,
                        params.amountIn,
                        "exactInput",
                    )
                }
                Err(_) => {
                    return Ok(DecodedTx {
                        amount_out_min: U256::ZERO,
                        path: vec![],
                        to: Address::ZERO,
                        deadline: U256::ZERO,
                        effective_value: tx_value,
                        method: "Unknown".to_string(),
                    })
                }
            }
        }

        _ => {
            return Ok(DecodedTx {
                amount_out_min: U256::ZERO,
                path: vec![],
                to: Address::ZERO,
                deadline: U256::ZERO,
                effective_value: tx_value,
                method: "Unknown".to_string(),
            });
        }
    };

    let effective_value = if tx_value > amount_in {
        tx_value
    } else {
        amount_in
    };

    Ok(DecodedTx {
        amount_out_min,
        path,
        to,
        deadline,
        effective_value,
        method: method.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_swap_exact_eth_for_tokens() {
        let input = vec![0x7f, 0xf3, 0x6a, 0xb5, 0x00, 0x00, 0x00, 0x00];
        assert!(is_target_transaction(&input));
    }

    #[test]
    fn test_exact_input_single_selector() {
        let input = vec![0x41, 0x4b, 0xf3, 0x89, 0x00, 0x00, 0x00, 0x00];
        assert!(is_target_transaction(&input));
    }

    #[test]
    fn test_multicall_v3_selector() {
        let input = vec![0x5a, 0xe4, 0x01, 0xdc, 0x00, 0x00, 0x00, 0x00];
        assert!(is_target_transaction(&input));
    }

    #[test]
    fn test_decode_v2_with_eth_value() {
        let calldata = hex::decode(
            "7ff36ab500000000000000000000000000000000000000000000000000000000000003e8\
             0000000000000000000000000000000000000000000000000000000000000080\
             000000000000000000000000742d35cc6634c0532925a3b844bc9e7595f0beb0\
             000000000000000000000000000000000000000000000000000000006555a3a0\
             0000000000000000000000000000000000000000000000000000000000000002\
             000000000000000000000000c02aaa39b223fe8d0a0e5c4f27ead9083c756cc2\
             000000000000000000000000dac17f958d2ee523a2206206994597c13d831ec7",
        )
        .unwrap();

        let tx_value = U256::from(1_000_000_000_000_000_000u128);
        let decoded = decode_transaction(&calldata, tx_value).unwrap();

        assert_eq!(decoded.effective_value, tx_value);
        assert_eq!(decoded.amount_out_min, U256::from(1000u64));
        assert_eq!(decoded.method, "swapExactETHForTokens");
    }

    #[test]
    fn test_decode_v2_tokens_with_amount_in() {
        let calldata = hex::decode(
            "18cbafe5\
             0000000000000000000000000000000000000000000000000000000000001388\
             00000000000000000000000000000000000000000000000000000000000003e8\
             00000000000000000000000000000000000000000000000000000000000000a0\
             000000000000000000000000742d35cc6634c0532925a3b844bc9e7595f0beb0\
             000000000000000000000000000000000000000000000000000000006555a3a0\
             0000000000000000000000000000000000000000000000000000000000000002\
             000000000000000000000000dac17f958d2ee523a2206206994597c13d831ec7\
             000000000000000000000000c02aaa39b223fe8d0a0e5c4f27ead9083c756cc2",
        )
        .unwrap();

        let tx_value = U256::ZERO;
        let decoded = decode_transaction(&calldata, tx_value).unwrap();

        assert_eq!(decoded.effective_value, U256::from(5000u64));
        assert_eq!(decoded.method, "swapExactTokensForETH");
    }

    #[test]
    fn test_decode_fail_open() {
        let invalid_data = vec![0x7f, 0xf3, 0x6a, 0xb5, 0x00, 0x00];
        let tx_value = U256::from(1_000_000_000_000_000_000u128);

        let decoded = decode_transaction(&invalid_data, tx_value);
        assert!(decoded.is_ok());
        let decoded = decoded.unwrap();
        assert_eq!(decoded.effective_value, tx_value);
        assert_eq!(decoded.method, "Unknown");
    }
}
