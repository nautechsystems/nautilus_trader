#!/usr/bin/env python3
"""
TQQQ 포지션 청산 스크립트
Paper Trading 계좌에서 TQQQ 전량 매도
"""

from ib_insync import IB, Stock, MarketOrder
import time


def close_tqqq_position():
    ib = IB()

    try:
        # Paper Trading 포트에 연결
        print("IBKR Gateway 연결 중...")
        ib.connect('127.0.0.1', 4002, clientId=99)
        print("연결 성공!")

        # 현재 포지션 확인
        positions = ib.positions()
        tqqq_position = None

        for pos in positions:
            if pos.contract.symbol == 'TQQQ':
                tqqq_position = pos
                break

        if tqqq_position is None:
            print("TQQQ 포지션 없음")
            return False

        qty = int(tqqq_position.position)
        print(f"TQQQ 포지션 발견: {qty}주")

        if qty <= 0:
            print("매도할 포지션 없음")
            return False

        # TQQQ 계약 생성
        contract = Stock('TQQQ', 'SMART', 'USD')
        ib.qualifyContracts(contract)

        # 시장가 매도 주문
        print(f"TQQQ {qty}주 시장가 매도 주문 중...")
        order = MarketOrder('SELL', qty)
        trade = ib.placeOrder(contract, order)

        # 체결 대기
        print("체결 대기 중...")
        timeout = 30
        start = time.time()

        while not trade.isDone():
            ib.sleep(0.5)
            if time.time() - start > timeout:
                print(f"타임아웃! 주문 상태: {trade.orderStatus.status}")
                break

        if trade.orderStatus.status == 'Filled':
            avg_price = trade.orderStatus.avgFillPrice
            print(f"청산 완료! 평균가: ${avg_price:.2f}")
            print(f"총 매도금액: ${qty * avg_price:,.2f}")
            return True
        else:
            print(f"주문 상태: {trade.orderStatus.status}")
            return False

    except Exception as e:
        print(f"오류: {e}")
        return False
    finally:
        if ib.isConnected():
            ib.disconnect()
            print("연결 해제")


if __name__ == "__main__":
    success = close_tqqq_position()
    exit(0 if success else 1)
