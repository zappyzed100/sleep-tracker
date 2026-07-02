# File: src_python/analyzer.py
# Description: Predictive models for estimating sleep duration based on start time.
# Date: 2026-06-27
# Author: Antigravity
# Main Functions: predict_sleep_duration, train_model_if_needed, extract_features
# Dependencies: numpy, pandas, scikit-learn, datetime

import numpy as np
import pandas as pd
from datetime import datetime
from sklearn.ensemble import RandomForestRegressor
from sklearn.linear_model import Ridge
import os

# 最小限必要なデータポイント数
ML_MODEL_MIN_SAMPLES = 10

def get_time_features(dt: datetime) -> tuple[float, float, int]:
    """日時オブジェクトから、時刻の周期特徴量 (sin, cos) と曜日を取得する"""
    hour_float = dt.hour + dt.minute / 60.0 + dt.second / 3600.0
    # 24時間周期の角度
    angle = 2.0 * np.pi * hour_float / 24.0
    sin_time = np.sin(angle)
    cos_time = np.cos(angle)
    day_of_week = dt.weekday() # 0: 月曜日, 6: 日曜日
    return sin_time, cos_time, day_of_week

def predict_sleep_duration(sessions: list, current_dt: datetime) -> tuple[float, str]:
    """
    過去のセッションリストと現在の開始時刻から、予測される睡眠時間（時間数）を計算する。
    戻り値: (予測睡眠時間[時間], 予測に使用したアルゴリズム名)
    """
    if not sessions:
        # データが一切ない場合のデフォルト値（一般的な睡眠時間 7.5時間）
        return 7.5, "Default (No Data)"

    # セッションリストを DataFrame に変換
    df = pd.DataFrame(sessions, columns=['start_time', 'end_time', 'duration_hours', 'session_type'])
    df['start_dt'] = pd.to_datetime(df['start_time'])
    
    # サンプル数が少ない場合は、単純な統計モデル（ヒューリスティック）を使用する
    if len(df) < ML_MODEL_MIN_SAMPLES:
        return predict_with_heuristics(df, current_dt)

    # 機械学習モデルによる予測
    return predict_with_ml(df, current_dt)

def predict_with_heuristics(df: pd.DataFrame, current_dt: datetime) -> tuple[float, str]:
    """過去の同時刻帯（±2時間以内）の睡眠データの平均を返す"""
    current_time_float = current_dt.hour + current_dt.minute / 60.0
    
    # 各過去セッションの開始時刻（浮動小数点）を計算
    df['start_hour_float'] = df['start_dt'].dt.hour + df['start_dt'].dt.minute / 60.0
    
    # 24時間を循環させた時間差を計算
    def get_time_diff(h1, h2):
        diff = abs(h1 - h2)
        return np.minimum(diff, 24 - diff)
    
    df['time_diff'] = get_time_diff(df['start_hour_float'], current_time_float)
    
    # 開始時刻が近い（2時間以内）のデータを抽出
    close_sessions = df[df['time_diff'] <= 2.0]
    
    if len(close_sessions) >= 3:
        pred_duration = float(close_sessions['duration_hours'].mean())
        if not (pred_duration == pred_duration):  # NaN check
            pred_duration = 7.5
        return pred_duration, f"Heuristic (Average of {len(close_sessions)} similar time-of-day sessions)"
    else:
        # 近い時間帯のデータが少なすぎる場合は、全体の平均値（外れ値を除外）を返す
        q_low = df['duration_hours'].quantile(0.1)
        q_high = df['duration_hours'].quantile(0.9)
        filtered_df = df[(df['duration_hours'] >= q_low) & (df['duration_hours'] <= q_high)]
        if filtered_df.empty:
            filtered_df = df
        pred_duration = float(filtered_df['duration_hours'].mean())
        if not (pred_duration == pred_duration):  # NaN check
            pred_duration = 7.5
        return pred_duration, "Heuristic (Global average, trimmed)"

def predict_with_ml(df: pd.DataFrame, current_dt: datetime) -> tuple[float, str]:
    """Random Forest Regressor を使用した睡眠時間の回帰予測 (連続覚醒時間を特徴量に追加)"""
    df = df.copy()
    df['end_dt'] = pd.to_datetime(df['end_time'])
    
    # 過去セッションの連続覚醒時間を計算
    # i 番目のセッションの覚醒時間 = start_dt[i] - end_dt[i-1]
    awake_durations = []
    for i in range(len(df)):
        if i == 0:
            awake_durations.append(None)
        else:
            prev_end = df['end_dt'].iloc[i-1]
            curr_start = df['start_dt'].iloc[i]
            dur = (curr_start - prev_end).total_seconds() / 3600.0
            # 異常値 (旅行やPCの長期不使用などによる長期ギャップ) のクリップ
            if dur < 0.0 or dur > 48.0:
                awake_durations.append(None)
            else:
                awake_durations.append(dur)
                
    # 欠損値を平均値（またはデフォルト16時間）で補完
    valid_durs = [d for d in awake_durations if d is not None]
    mean_dur = np.mean(valid_durs) if valid_durs else 16.0
    mean_dur = max(4.0, min(24.0, mean_dur)) # 常識的な範囲にクリップ
    awake_durations = [d if d is not None else mean_dur for d in awake_durations]
    df['awake_duration'] = awake_durations

    X = []
    y = []
    
    for _, row in df.iterrows():
        s_dt = row['start_dt']
        sin_t, cos_t, dow = get_time_features(s_dt)
        # 曜日を One-Hot 表現にするための7要素のリスト
        dow_onehot = [0] * 7
        dow_onehot[dow] = 1
        
        # 特徴量ベクトル: [sin_time, cos_time, awake_duration] + One-Hot 曜日
        features = [sin_t, cos_t, row['awake_duration']] + dow_onehot
        X.append(features)
        y.append(row['duration_hours'])
        
    X = np.array(X)
    y = np.array(y)
    
    # モデルの訓練
    model = RandomForestRegressor(n_estimators=50, random_state=42)
    model.fit(X, y)
    
    # 予測対象の特徴量作成 (入眠仮定時刻 - 最後の起床時刻)
    last_end = df['end_dt'].iloc[-1]
    c_awake_dur = (current_dt - last_end).total_seconds() / 3600.0
    c_awake_dur = max(0.0, min(48.0, c_awake_dur)) # 0〜48時間にクリップ
    
    c_sin_t, c_cos_t, c_dow = get_time_features(current_dt)
    c_dow_onehot = [0] * 7
    c_dow_onehot[c_dow] = 1
    
    # 特徴量ベクトル: [sin_time, cos_time, awake_duration] + One-Hot 曜日
    c_features = np.array([[c_sin_t, c_cos_t, c_awake_dur] + c_dow_onehot])
    
    pred_duration = float(model.predict(c_features)[0])
    
    # 結果のバリデーション (現実的な範囲 1〜18時間にクリップ)
    pred_duration = max(1.0, min(18.0, pred_duration))
    
    return pred_duration, f"Machine Learning (Random Forest with Awake Duration: {c_awake_dur:.1f}h)"

