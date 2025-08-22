#!/usr/bin/env python3
"""
Nautilus Trader 超参数优化系统 - 融合FreqTrade设计理念
Enhanced Hyperparameter Optimization System inspired by FreqTrade
"""

import json
import pickle
from abc import ABC, abstractmethod
from concurrent.futures import ProcessPoolExecutor, as_completed
from dataclasses import dataclass, field
from datetime import datetime
from pathlib import Path
from typing import Any, Dict, List, Optional, Tuple, Union, Sequence
import numpy as np
import pandas as pd
import warnings

from nautilus_trader.backtest.config import BacktestRunConfig
from nautilus_trader.backtest.node import BacktestNode
from nautilus_trader.trading.config import ImportableStrategyConfig

# 可选依赖
try:
    import optuna
    OPTUNA_AVAILABLE = True
except ImportError:
    OPTUNA_AVAILABLE = False
    warnings.warn("Optuna not available. Install with: pip install optuna")


# ============================================================================
# 1. 参数定义系统 (Parameter Definition System) - FreqTrade兼容
# ============================================================================

@dataclass
class BaseParameter:
    """参数基类"""
    name: str = ""
    space: str = "strategy"  # "buy", "sell", "protection", "strategy"
    optimize: bool = True
    
    def __post_init__(self):
        if not self.name:
            raise ValueError("Parameter name cannot be empty")


class DecimalParameter(BaseParameter):
    """浮点数参数 - FreqTrade兼容接口"""
    def __init__(self, low: float, high: float, *, default: float,
                 space: str = "strategy", optimize: bool = True, decimals: int = 3):
        self.low = low
        self.high = high
        self.default = default
        self.decimals = decimals
        self.value = default
        super().__init__(space=space, optimize=optimize)
    
    def suggest(self, trial) -> float:
        """Optuna试验建议值"""
        if OPTUNA_AVAILABLE:
            return trial.suggest_float(self.name, self.low, self.high)
        else:
            return np.random.uniform(self.low, self.high)


class IntParameter(BaseParameter):
    """整数参数 - FreqTrade兼容接口"""
    def __init__(self, low: int, high: int, *, default: int,
                 space: str = "strategy", optimize: bool = True):
        self.low = low
        self.high = high
        self.default = default
        self.value = default
        super().__init__(space=space, optimize=optimize)
    
    def suggest(self, trial) -> int:
        """Optuna试验建议值"""
        if OPTUNA_AVAILABLE:
            return trial.suggest_int(self.name, self.low, self.high)
        else:
            return np.random.randint(self.low, self.high + 1)


class CategoricalParameter(BaseParameter):
    """分类参数 - FreqTrade兼容接口"""
    def __init__(self, categories: Sequence[Any], *, default: Any = None,
                 space: str = "strategy", optimize: bool = True):
        self.categories = list(categories)
        self.default = default if default is not None else categories[0]
        self.value = self.default
        super().__init__(space=space, optimize=optimize)
    
    def suggest(self, trial) -> Any:
        """Optuna试验建议值"""
        if OPTUNA_AVAILABLE:
            return trial.suggest_categorical(self.name, self.categories)
        else:
            return np.random.choice(self.categories)


# ============================================================================
# 2. 损失函数系统 (Loss Function System) - FreqTrade兼容
# ============================================================================

class IHyperOptLoss(ABC):
    """超参数优化损失函数接口 - FreqTrade兼容"""
    
    @staticmethod
    @abstractmethod
    def hyperopt_loss_function(
        results: pd.DataFrame,
        trade_count: int,
        min_date: datetime,
        max_date: datetime,
        config: Dict[str, Any],
        processed: Dict[str, Any],
        backtest_stats: Dict[str, Any],
        starting_balance: float,
        *args, **kwargs
    ) -> float:
        """
        计算损失值 - 返回值越小表示结果越好
        
        Parameters
        ----------
        results : pd.DataFrame
            交易结果数据
        trade_count : int
            交易数量
        min_date : datetime
            回测开始日期
        max_date : datetime
            回测结束日期
        config : Dict[str, Any]
            配置信息
        processed : Dict[str, Any]
            处理后的数据
        backtest_stats : Dict[str, Any]
            回测统计信息
        starting_balance : float
            起始余额
            
        Returns
        -------
        float
            损失值 (越小越好)
        """
        pass


class SharpeHyperOptLoss(IHyperOptLoss):
    """夏普比率损失函数"""
    
    @staticmethod
    def hyperopt_loss_function(results: pd.DataFrame, **kwargs) -> float:
        if results.empty:
            return 1000.0  # 惩罚无交易
        
        # 从backtest_stats中获取夏普比率
        backtest_stats = kwargs.get('backtest_stats', {})
        sharpe_ratio = backtest_stats.get('sharpe_ratio', 0.0)
        
        # 返回负值，因为我们要最小化损失
        return -sharpe_ratio


class ProfitHyperOptLoss(IHyperOptLoss):
    """利润损失函数"""
    
    @staticmethod
    def hyperopt_loss_function(results: pd.DataFrame, **kwargs) -> float:
        if results.empty:
            return 1000.0
        
        backtest_stats = kwargs.get('backtest_stats', {})
        total_return = backtest_stats.get('total_return', 0.0)
        
        return -total_return  # 负值：利润越高，损失越小


class MultiMetricHyperOptLoss(IHyperOptLoss):
    """多指标综合损失函数"""
    
    @staticmethod
    def hyperopt_loss_function(results: pd.DataFrame, trade_count: int, **kwargs) -> float:
        if results.empty:
            return 1000.0
        
        backtest_stats = kwargs.get('backtest_stats', {})
        
        # 获取各项指标
        total_return = backtest_stats.get('total_return', 0.0)
        sharpe_ratio = backtest_stats.get('sharpe_ratio', 0.0)
        max_drawdown = backtest_stats.get('max_drawdown', 0.0)
        win_rate = backtest_stats.get('win_rate', 0.0)
        
        # 交易数量惩罚
        TARGET_TRADES = 100
        trade_penalty = abs(trade_count - TARGET_TRADES) / TARGET_TRADES
        
        # 综合评分 (权重可调整)
        score = (
            0.4 * total_return +           # 40% 总收益
            0.3 * sharpe_ratio +           # 30% 夏普比率  
            0.2 * (1 - abs(max_drawdown)) + # 20% 回撤控制
            0.1 * win_rate -               # 10% 胜率
            0.1 * trade_penalty            # 交易数量惩罚
        )
        
        return -score  # 返回负值


# ============================================================================
# 3. 优化器配置
# ============================================================================

@dataclass
class HyperOptConfig:
    """超参数优化配置 - FreqTrade风格"""
    strategy_config: ImportableStrategyConfig
    backtest_config: BacktestRunConfig
    parameters: Dict[str, BaseParameter]
    loss_function: IHyperOptLoss = field(default_factory=SharpeHyperOptLoss)
    
    # 优化设置
    epochs: int = 100
    jobs: int = 1
    random_state: int = 42
    
    # Optuna设置
    sampler: str = "TPESampler"  # TPESampler, RandomSampler, CmaEsSampler
    n_startup_trials: int = 30
    
    # 早停设置
    early_stopping: bool = False
    early_stopping_patience: int = 50
    early_stopping_threshold: float = 0.001
    
    # 结果保存
    results_dir: str = "hyperopt_results"
    save_trials: bool = True


# ============================================================================
# 4. 主优化器类
# ============================================================================

class NautilusHyperOpt:
    """Nautilus Trader 超参数优化器 - FreqTrade兼容设计"""
    
    def __init__(self, config: HyperOptConfig):
        self.config = config
        self.study: Optional[Any] = None
        self.best_params: Optional[Dict[str, Any]] = None
        self.best_loss: Optional[float] = None
        self.trials_results: List[Dict[str, Any]] = []
        
        # 创建结果目录
        Path(self.config.results_dir).mkdir(exist_ok=True)
        
        # 设置参数名称
        for name, param in self.config.parameters.items():
            param.name = name
    
    def start(self) -> Dict[str, Any]:
        """开始优化 - 主入口点"""
        print(f"开始超参数优化: {self.config.epochs} epochs, {self.config.jobs} jobs")
        
        if OPTUNA_AVAILABLE and self.config.jobs == 1:
            return self._optimize_with_optuna()
        else:
            return self._optimize_with_parallel()
    
    def _optimize_with_optuna(self) -> Dict[str, Any]:
        """使用Optuna进行优化"""
        # 创建采样器
        if self.config.sampler == "TPESampler":
            sampler = optuna.samplers.TPESampler(
                seed=self.config.random_state,
                n_startup_trials=self.config.n_startup_trials
            )
        elif self.config.sampler == "RandomSampler":
            sampler = optuna.samplers.RandomSampler(seed=self.config.random_state)
        elif self.config.sampler == "CmaEsSampler":
            sampler = optuna.samplers.CmaEsSampler(seed=self.config.random_state)
        else:
            sampler = optuna.samplers.TPESampler(seed=self.config.random_state)
        
        # 创建研究
        self.study = optuna.create_study(
            direction="minimize",
            sampler=sampler
        )
        
        # 早停回调
        callbacks = []
        if self.config.early_stopping:
            callbacks.append(self._early_stopping_callback)
        
        # 执行优化
        self.study.optimize(
            self._objective_function,
            n_trials=self.config.epochs,
            callbacks=callbacks,
            show_progress_bar=True
        )
        
        # 处理结果
        self.best_params = self.study.best_params
        self.best_loss = self.study.best_value
        
        return self._format_results()
    
    def _objective_function(self, trial) -> float:
        """Optuna目标函数"""
        # 生成参数
        params = {}
        for name, param in self.config.parameters.items():
            if param.optimize:
                params[name] = param.suggest(trial)
            else:
                params[name] = param.default
        
        # 执行回测
        result = self._run_single_backtest(params)
        
        # 保存试验结果
        trial_result = {
            "trial_number": trial.number,
            "params": params,
            "loss": result["loss"],
            "metrics": result.get("metrics", {}),
            "datetime": datetime.now().isoformat()
        }
        self.trials_results.append(trial_result)
        
        return result["loss"]

    def _optimize_with_parallel(self) -> Dict[str, Any]:
        """使用并行处理进行优化"""
        # 生成所有参数组合
        param_combinations = self._generate_parameter_combinations()

        print(f"并行优化: {len(param_combinations)} 个参数组合")

        if self.config.jobs == 1:
            # 串行执行
            results = []
            for i, params in enumerate(param_combinations):
                print(f"执行回测 {i+1}/{len(param_combinations)}")
                result = self._run_single_backtest(params)
                results.append(result)
        else:
            # 并行执行
            with ProcessPoolExecutor(max_workers=self.config.jobs) as executor:
                futures = {
                    executor.submit(self._run_single_backtest, params): i
                    for i, params in enumerate(param_combinations)
                }

                results = []
                for future in as_completed(futures):
                    i = futures[future]
                    print(f"完成回测 {i+1}/{len(param_combinations)}")
                    result = future.result()
                    results.append(result)

        # 处理结果
        self.trials_results = results
        best_result = min(results, key=lambda x: x["loss"])
        self.best_params = best_result["params"]
        self.best_loss = best_result["loss"]

        return self._format_results()

    def _generate_parameter_combinations(self) -> List[Dict[str, Any]]:
        """生成参数组合"""
        import itertools

        param_grids = []
        param_names = []

        for name, param in self.config.parameters.items():
            if not param.optimize:
                continue

            param_names.append(name)

            if isinstance(param, CategoricalParameter):
                param_grids.append(param.categories)
            elif isinstance(param, IntParameter):
                # 生成整数范围
                values = list(range(param.low, param.high + 1))
                # 如果范围太大，采样
                if len(values) > 20:
                    step = max(1, (param.high - param.low) // 20)
                    values = list(range(param.low, param.high + 1, step))
                param_grids.append(values)
            elif isinstance(param, DecimalParameter):
                # 生成浮点数范围
                values = list(np.linspace(param.low, param.high, 10))
                param_grids.append(values)

        # 生成所有组合
        if not param_grids:
            return [{}]

        combinations = list(itertools.product(*param_grids))

        # 限制组合数量
        if len(combinations) > self.config.epochs:
            np.random.seed(self.config.random_state)
            indices = np.random.choice(len(combinations), self.config.epochs, replace=False)
            combinations = [combinations[i] for i in indices]

        # 转换为字典格式
        param_combinations = []
        for combo in combinations:
            params = dict(zip(param_names, combo))
            # 添加非优化参数
            for name, param in self.config.parameters.items():
                if not param.optimize:
                    params[name] = param.default
            param_combinations.append(params)

        return param_combinations

    def _run_single_backtest(self, params: Dict[str, Any]) -> Dict[str, Any]:
        """执行单次回测"""
        try:
            # 更新策略配置
            updated_strategy_config = self.config.strategy_config.model_copy()
            updated_strategy_config.config.update(params)

            # 更新回测配置
            updated_backtest_config = self.config.backtest_config.model_copy()
            updated_backtest_config.strategies = [updated_strategy_config]

            # 执行回测
            node = BacktestNode([updated_backtest_config])
            results = node.run()

            if not results:
                return {"params": params, "loss": 1000.0, "error": "No results"}

            result = results[0]

            # 提取统计信息
            stats = result.stats_returns

            # 构建回测统计信息
            backtest_stats = {
                "total_return": stats.get("Total Return [%]", 0.0),
                "sharpe_ratio": stats.get("Sharpe Ratio", 0.0),
                "max_drawdown": stats.get("Max Drawdown [%]", 0.0),
                "win_rate": stats.get("Win Rate [%]", 0.0),
                "profit_factor": stats.get("Profit Factor", 0.0),
                "total_trades": stats.get("# Trades", 0),
            }

            # 计算损失
            loss = self.config.loss_function.hyperopt_loss_function(
                results=pd.DataFrame(),  # 简化版本
                trade_count=backtest_stats["total_trades"],
                min_date=datetime.now(),
                max_date=datetime.now(),
                config={},
                processed={},
                backtest_stats=backtest_stats,
                starting_balance=1000000.0
            )

            return {
                "params": params,
                "loss": loss,
                "metrics": backtest_stats,
                "success": True
            }

        except Exception as e:
            return {
                "params": params,
                "loss": 1000.0,
                "error": str(e),
                "success": False
            }

    def _early_stopping_callback(self, study, trial):
        """早停回调函数"""
        if len(study.trials) < self.config.early_stopping_patience:
            return

        # 检查最近的试验是否有改进
        recent_trials = study.trials[-self.config.early_stopping_patience:]
        recent_values = [t.value for t in recent_trials if t.value is not None]

        if len(recent_values) < self.config.early_stopping_patience:
            return

        # 计算改进幅度
        best_recent = min(recent_values)
        best_overall = study.best_value

        improvement = abs(best_overall - best_recent) / abs(best_overall) if best_overall != 0 else 0

        if improvement < self.config.early_stopping_threshold:
            print(f"早停触发: 最近 {self.config.early_stopping_patience} 次试验改进小于 {self.config.early_stopping_threshold}")
            study.stop()

    def _format_results(self) -> Dict[str, Any]:
        """格式化结果"""
        successful_trials = [t for t in self.trials_results if t.get("success", True)]

        results = {
            "best_params": self.best_params,
            "best_loss": self.best_loss,
            "total_trials": len(self.trials_results),
            "successful_trials": len(successful_trials),
            "optimization_method": self.config.sampler if OPTUNA_AVAILABLE else "parallel"
        }

        # 保存结果
        if self.config.save_trials:
            self._save_results()

        return results

    def _save_results(self):
        """保存优化结果"""
        timestamp = datetime.now().strftime("%Y%m%d_%H%M%S")

        # 保存详细结果
        results_file = Path(self.config.results_dir) / f"hyperopt_results_{timestamp}.json"
        with open(results_file, 'w') as f:
            json.dump(self.trials_results, f, indent=2, default=str)

        # 保存最佳参数
        best_params_file = Path(self.config.results_dir) / f"best_params_{timestamp}.json"
        with open(best_params_file, 'w') as f:
            json.dump({
                "best_params": self.best_params,
                "best_loss": self.best_loss,
                "sampler": self.config.sampler
            }, f, indent=2)

        # 保存Optuna研究对象
        if self.study is not None:
            study_file = Path(self.config.results_dir) / f"optuna_study_{timestamp}.pkl"
            with open(study_file, 'wb') as f:
                pickle.dump(self.study, f)

        print(f"结果已保存到: {self.config.results_dir}")

    def get_results_dataframe(self) -> pd.DataFrame:
        """获取结果DataFrame"""
        data = []
        for trial in self.trials_results:
            if trial.get("success", True):
                row = trial["params"].copy()
                if "metrics" in trial:
                    row.update(trial["metrics"])
                row["loss"] = trial["loss"]
                data.append(row)

        return pd.DataFrame(data)
