// This is free and unencumbered software released into the public domain.

use alloy::sol;

sol! {
    /// See: <https://docs.openzeppelin.com/contracts/5.x/api/token/erc20>
    #[derive(Debug, PartialEq)]
    #[sol(rpc)]
    interface Erc20 {
        function totalSupply() external view returns (uint256);
        function balanceOf(address account) external view returns (uint256);
        function transfer(address to, uint256 value) external view returns (bool);
        function allowance(address owner, address spender) external view returns (uint256);
        function approve(address spender, uint256 value) external view returns (bool);
        function transferFrom(address from, address to, uint256 value) external view returns (bool);
        function name() external view returns (string);
        function symbol() external view returns (string);
        function decimals() external view returns (uint8);
    }
}
