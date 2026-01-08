# Kaggle API 설정 방법

## 1단계: Kaggle 계정 생성/로그인
https://www.kaggle.com/ 에서 로그인

## 2단계: API 토큰 생성
1. https://www.kaggle.com/settings 이동
2. "API" 섹션에서 "Create New Token" 클릭
3. `kaggle.json` 파일이 다운로드됨

## 3단계: 토큰 설치
```bash
mkdir -p ~/.kaggle
mv ~/Downloads/kaggle.json ~/.kaggle/
chmod 600 ~/.kaggle/kaggle.json
```

## 4단계: 데이터셋 다운로드
```bash
cd /Users/clogic/Workspace/Trading/nautilus/my_strategies/data

# SPY 옵션 데이터 (2019-2022)
kaggle datasets download -d kylegraupe/spy-daily-eod-options-quotes-2020-2022
unzip spy-daily-eod-options-quotes-2020-2022.zip -d spy_options

# VIX Daily
kaggle datasets download -d guillemservera/vix-cboe-volatility-index-daily-updated
unzip vix-cboe-volatility-index-daily-updated.zip -d vix_daily
```

## 다운로드할 데이터셋
1. `kylegraupe/spy-daily-eod-options-quotes-2020-2022` - SPY 옵션 EOD (가장 중요!)
2. `guillemservera/vix-cboe-volatility-index-daily-updated` - VIX 일별
3. `jasonli88/spyoptionstradetickerdata` - SPY 옵션 거래 데이터
