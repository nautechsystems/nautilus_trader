#!/usr/bin/env python3
"""
Nautilus Trader 超参数优化使用示例
FreqTrade兼容风格的参数优化演示
"""

from decimal import Decimal
from datetime import datetime
import pandas as pd

from nautilus_trader.backtest.config import BacktestRunConfig, BacktestVenueConfig
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.enums import AccountType, OmsType
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.trading.config import ImportableStrategyConfig

from nautilus_hyperopt import (
    NautilusHyperOpt,
    HyperOptConfig,
    DecimalParameter,
    IntParameter,
    CategoricalParameter,
    SharpeHyperOptLoss,
    ProfitHyperOptLoss,
    MultiMetricHyperOptLoss
)


def create_freqtrade_style_optimization():
    """创建FreqTrade风格的优化配置"""
    
    # 1. 定义策略配置
    strategy_config = ImportableStrategyConfig(
        strategy_path="nautilus_trader.examples.strategies.ema_cross.EMACross",
        config_path="nautilus_trader.examples.strategies.ema_cross.EMACrossConfig",
        config={
            "instrument_id": "EUR/USD.SIM",
            "bar_type": "EUR/USD.SIM-5-MINUTE-BID-INTERNAL",
            "trade_size": Decimal("100000"),
            # 以下参数将被优化
            "fast_ema_period": 10,
            "slow_ema_period": 20,
        }
    )
    
    # 2. 定义回测配置
    backtest_config = BacktestRunConfig(
        engine={
            "trader_id": TraderId("BACKTESTER-001"),
        },
        venues=[
            BacktestVenueConfig(
                name="SIM",
                oms_type=OmsType.NETTING,
                account_type=AccountType.MARGIN,
                starting_balances=["1000000 USD"],
                base_currency="USD",
            )
        ],
        data=[
            # 实际使用时需要配置数据源
        ],
        strategies=[strategy_config],
        start="2023-01-01",
        end="2023-12-31",
    )
    
    # 3. 定义参数空间 - FreqTrade风格
    parameters = {
        # 买入信号参数
        "fast_ema_period": IntParameter(
            low=5, high=20, default=10, 
            space="buy", optimize=True
        ),
        "slow_ema_period": IntParameter(
            low=20, high=50, default=20,
            space="buy", optimize=True
        ),
        
        # 交易参数
        "trade_size": CategoricalParameter(
            categories=[Decimal("50000"), Decimal("100000"), Decimal("200000")],
            default=Decimal("100000"),
            space="strategy", optimize=True
        ),
        
        # 风险管理参数 (示例)
        "stop_loss_pct": DecimalParameter(
            low=0.01, high=0.05, default=0.02,
            space="protection", optimize=False  # 暂不优化
        ),
    }
    
    # 4. 创建优化配置
    hyperopt_config = HyperOptConfig(
        strategy_config=strategy_config,
        backtest_config=backtest_config,
        parameters=parameters,
        loss_function=SharpeHyperOptLoss(),  # 使用夏普比率作为目标
        
        # 优化设置
        epochs=100,
        jobs=4,  # 并行进程数
        random_state=42,
        
        # Optuna设置
        sampler="TPESampler",  # 贝叶斯优化
        n_startup_trials=20,
        
        # 早停设置
        early_stopping=True,
        early_stopping_patience=30,
        early_stopping_threshold=0.001,
        
        # 结果保存
        results_dir="freqtrade_style_results",
        save_trials=True
    )
    
    return hyperopt_config


def run_basic_optimization():
    """运行基础优化示例"""
    print("=== FreqTrade风格超参数优化示例 ===")
    
    # 创建配置
    config = create_freqtrade_style_optimization()
    
    # 创建优化器
    hyperopt = NautilusHyperOpt(config)
    
    # 执行优化
    print("开始执行超参数优化...")
    results = hyperopt.start()
    
    # 显示结果
    print("\n=== 优化结果 ===")
    print(f"最佳参数: {results['best_params']}")
    print(f"最佳损失值: {results['best_loss']:.6f}")
    print(f"总试验次数: {results['total_trials']}")
    print(f"成功试验次数: {results['successful_trials']}")
    print(f"优化方法: {results['optimization_method']}")
    
    # 获取详细结果
    df = hyperopt.get_results_dataframe()
    if not df.empty:
        print("\n=== 前10个最佳结果 ===")
        df_sorted = df.sort_values('loss', ascending=True)
        print(df_sorted.head(10).to_string(index=False))


def run_different_loss_functions():
    """演示不同损失函数的使用"""
    base_config = create_freqtrade_style_optimization()
    
    loss_functions = [
        ("Sharpe Ratio", SharpeHyperOptLoss()),
        ("Profit", ProfitHyperOptLoss()),
        ("Multi-Metric", MultiMetricHyperOptLoss()),
    ]
    
    for name, loss_func in loss_functions:
        print(f"\n=== 使用 {name} 损失函数优化 ===")
        
        # 更新配置
        config = base_config
        config.loss_function = loss_func
        config.results_dir = f"results_{name.lower().replace(' ', '_')}"
        config.epochs = 30  # 减少迭代次数用于演示
        
        try:
            # 创建优化器并运行
            hyperopt = NautilusHyperOpt(config)
            results = hyperopt.start()
            
            print(f"最佳参数: {results['best_params']}")
            print(f"最佳损失值: {results['best_loss']:.6f}")
            
        except Exception as e:
            print(f"优化失败: {e}")


def create_advanced_strategy_optimization():
    """创建高级策略优化示例"""
    
    # 更复杂的参数空间
    parameters = {
        # 技术指标参数
        "rsi_period": IntParameter(10, 30, default=14, space="buy"),
        "rsi_oversold": IntParameter(20, 40, default=30, space="buy"),
        "rsi_overbought": IntParameter(60, 80, default=70, space="sell"),
        
        # 移动平均参数
        "ma_type": CategoricalParameter(
            ["sma", "ema", "wma"], default="ema", space="buy"
        ),
        "ma_period": IntParameter(10, 50, default=20, space="buy"),
        
        # 布林带参数
        "bb_period": IntParameter(15, 25, default=20, space="buy"),
        "bb_std": DecimalParameter(1.5, 2.5, default=2.0, space="buy", decimals=1),
        
        # 风险管理参数
        "stop_loss": DecimalParameter(0.01, 0.05, default=0.02, space="protection"),
        "take_profit": DecimalParameter(0.02, 0.10, default=0.04, space="protection"),
        
        # 交易参数
        "min_roi": DecimalParameter(0.005, 0.02, default=0.01, space="strategy"),
        "timeframe": CategoricalParameter(
            ["1m", "5m", "15m", "1h"], default="5m", space="strategy"
        ),
    }
    
    return parameters


def analyze_optimization_results(results_dir: str):
    """分析优化结果"""
    import json
    from pathlib import Path
    import matplotlib.pyplot as plt
    
    results_path = Path(results_dir)
    
    # 查找结果文件
    json_files = list(results_path.glob("hyperopt_results_*.json"))
    if not json_files:
        print("未找到结果文件")
        return
    
    # 加载最新结果
    latest_file = max(json_files, key=lambda x: x.stat().st_mtime)
    with open(latest_file, 'r') as f:
        results = json.load(f)
    
    # 转换为DataFrame
    data = []
    for trial in results:
        if trial.get("success", True):
            row = trial["params"].copy()
            if "metrics" in trial:
                row.update(trial["metrics"])
            row["loss"] = trial["loss"]
            data.append(row)
    
    df = pd.DataFrame(data)
    
    if df.empty:
        print("没有成功的试验结果")
        return
    
    # 基础统计
    print(f"\n=== 优化结果分析 ===")
    print(f"总试验次数: {len(df)}")
    print(f"最佳损失值: {df['loss'].min():.6f}")
    print(f"平均损失值: {df['loss'].mean():.6f}")
    print(f"损失值标准差: {df['loss'].std():.6f}")
    
    # 最佳参数
    best_idx = df['loss'].idxmin()
    best_trial = df.loc[best_idx]
    print(f"\n最佳参数组合:")
    for col in df.columns:
        if col not in ['loss'] and 'metrics' not in col:
            print(f"  {col}: {best_trial[col]}")
    
    # 简单可视化
    try:
        plt.figure(figsize=(12, 8))
        
        # 损失值分布
        plt.subplot(2, 2, 1)
        plt.hist(df['loss'], bins=20, alpha=0.7)
        plt.xlabel('Loss Value')
        plt.ylabel('Frequency')
        plt.title('Loss Distribution')
        
        # 收敛曲线
        plt.subplot(2, 2, 2)
        cummin_loss = df['loss'].cummin()
        plt.plot(range(len(cummin_loss)), cummin_loss)
        plt.xlabel('Trial')
        plt.ylabel('Best Loss So Far')
        plt.title('Optimization Convergence')
        
        # 参数vs损失散点图 (选择数值参数)
        numeric_params = df.select_dtypes(include=[int, float]).columns
        numeric_params = [col for col in numeric_params if col != 'loss']
        
        if len(numeric_params) >= 2:
            plt.subplot(2, 2, 3)
            param1, param2 = numeric_params[0], numeric_params[1]
            scatter = plt.scatter(df[param1], df[param2], c=df['loss'], cmap='viridis')
            plt.xlabel(param1)
            plt.ylabel(param2)
            plt.title(f'{param1} vs {param2}')
            plt.colorbar(scatter)
        
        # 参数重要性 (简化版)
        if len(numeric_params) > 0:
            plt.subplot(2, 2, 4)
            correlations = df[numeric_params + ['loss']].corr()['loss'].abs().sort_values(ascending=False)
            correlations = correlations.drop('loss')[:5]  # 前5个最相关的参数
            
            plt.barh(range(len(correlations)), correlations.values)
            plt.yticks(range(len(correlations)), correlations.index)
            plt.xlabel('Correlation with Loss')
            plt.title('Parameter Importance')
        
        plt.tight_layout()
        plt.savefig(results_path / "optimization_analysis.png", dpi=300, bbox_inches='tight')
        plt.show()
        
    except Exception as e:
        print(f"可视化失败: {e}")


if __name__ == "__main__":
    # 运行基础优化示例
    run_basic_optimization()
    
    # 可选: 运行不同损失函数的比较
    # run_different_loss_functions()
    
    # 可选: 分析结果
    # analyze_optimization_results("freqtrade_style_results")
