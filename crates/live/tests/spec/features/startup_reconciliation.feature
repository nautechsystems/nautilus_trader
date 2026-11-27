Feature: Startup reconciliation
  When enabled, startup reconciliation recovers eligible venue orders and
  reconciles included position reports before actors and strategies start.
  An unresolved included position report blocks startup.

  The ETH scenarios use an ETH/USDT perpetual instrument and a USDT margin account.
  The ES scenarios use E-mini S&P 500 futures with a 0.25-point tick and a contract multiplier of 50 USD per index point.

  Background:
    Given startup reconciliation is enabled
    And position reports are included in reconciliation
    And missing-order generation is enabled
    And the cache has a trading instrument and account but no orders or positions

  Rule: NETTING startup exposes reconciled state before strategies start

    @netting @flat
    Scenario Outline: Start flat with an empty cache using <entrypoint>
      Given the execution client uses NETTING accounting
      And the venue reports no orders, fills, or positions
      When the live node starts using <entrypoint>
      Then startup succeeds
      And the strategy observes no open orders or positions when it starts
      And the portfolio exposes 0 ETH when the strategy starts
      And no position is cached
      And the actor and strategy each start once
      And the venue receives a startup mass-status request

      Examples:
        | entrypoint |
        | start      |
        | run        |

    @netting @recovery
    Scenario Outline: Recover a reported NETTING <side> position with an empty cache using <entrypoint>
      Given the execution client uses NETTING accounting
      And the venue reports a <side> position of 5.000 ETH
      And the position report has no venue position ID
      And its average opening price is 2500.25 USDT
      When the live node starts using <entrypoint>
      Then startup succeeds
      And the strategy observes a <side> position of 5.000 ETH at 2500.25 USDT when it starts
      And the portfolio exposes <signed_qty> ETH when the strategy starts
      And the actor and strategy each start once
      And the venue receives a startup mass-status request

      Examples:
        | side  | entrypoint | signed_qty |
        | long  | start      | 5.000      |
        | long  | run        | 5.000      |
        | short | start      | -5.000     |
        | short | run        | -5.000     |

  Rule: A late report after restart must not count a recovered execution twice
    A startup position snapshot can already include an execution whose Filled
    order report arrives later. Even after the node is running, recovering that
    order must leave the recovered position and portfolio exposure unchanged.

    @netting @recovery @late_report
    Scenario Outline: Receive an earlier execution report after recovering a <side> position at restart
      Given the execution client uses NETTING accounting
      And the venue reports a <side> position of 5.000 ETH
      And the position report has no venue position ID
      And its average opening price is 2500.25 USDT
      And the execution in the delayed Filled order report is already included in the position snapshot
      When the live node restarts and receives the delayed Filled order report after entering Running
      Then startup succeeds
      And the strategy observes a <side> position of 5.000 ETH at 2500.25 USDT when it starts
      And the portfolio exposes <signed_qty> ETH when the strategy starts
      And the delayed order is FILLED for 5.000 ETH at 2500.25 USDT
      And the recovered position and portfolio exposure stay unchanged after the delayed report
      And exactly one position is cached
      And the actor and strategy each start once

      Examples:
        | side  | signed_qty |
        | long  | 5.000      |
        | short | -5.000     |

  Rule: Included position reports must be reconciled before actors and strategies start

    @recovery
    Scenario Outline: Reconcile a reported <side> position using <entrypoint>
      Given the venue reports a <side> position of 5.000 ETH
      And its average opening price is 2500.25 USDT
      When the live node starts using <entrypoint>
      Then startup succeeds
      And the strategy observes a <side> position of 5.000 ETH at 2500.25 USDT when it starts
      And the portfolio exposes <signed_qty> ETH when the strategy starts
      And the actor and strategy each start once
      And the venue receives a startup mass-status request

      Examples:
        | side  | entrypoint | signed_qty |
        | long  | start      | 5.000      |
        | long  | run        | 5.000      |
        | short | start      | -5.000     |
        | short | run        | -5.000     |

    @hedging @recovery
    Scenario Outline: Recover both legs of a net-flat hedge using <entrypoint>
      Given the cache has the ESZ21 futures contract and a USD margin account
      And the scripted execution client uses HEDGING accounting
      And the venue reports a long position of 5 ESZ21 contracts at 4500.25 USD with venue position ID HEDGE-LONG
      And the venue reports a short position of 5 ESZ21 contracts at 4503.75 USD with venue position ID HEDGE-SHORT
      When the live node starts using <entrypoint>
      Then startup succeeds
      And the strategy observes both reported hedge legs when it starts
      And the portfolio exposes 0 ESZ21 contracts when the strategy starts
      And the actor and strategy each start once
      And the venue receives a startup mass-status request

      Examples:
        | entrypoint |
        | start      |
        | run        |

    @unrecoverable
    Scenario Outline: Block startup when the opening price is missing using <entrypoint>
      Given the venue reports a long position of 5.000 ETH
      And its average opening price is unavailable
      When the live node starts using <entrypoint>
      Then startup fails because the opening price is missing
      And no actor or strategy starts
      And no position is cached
      And the venue receives a startup mass-status request
      And the execution client is disconnected

      Examples:
        | entrypoint |
        | start      |
        | run        |

    @generation_disabled
    Scenario Outline: Block startup when missing-order generation is disabled using <entrypoint>
      Given the venue reports a long position of 5.000 ETH
      And its average opening price is 2500.25 USDT
      And missing-order generation is disabled
      When the live node starts using <entrypoint>
      Then startup fails because missing-order generation is disabled
      And no actor or strategy starts
      And no position is cached
      And the venue receives a startup mass-status request
      And the execution client is disconnected

      Examples:
        | entrypoint |
        | start      |
        | run        |

    @instrument_scope
    Scenario Outline: Recover an in-scope position alongside an excluded report using <entrypoint>
      Given the cache has the ESZ21 futures contract and a USD margin account
      And the execution client uses NETTING accounting
      And startup reconciliation is limited to ESZ21.GLBX
      And the venue reports a long position of 5 ESZ21 contracts
      And the position report has no venue position ID
      And its average opening price is 4500.25 USD
      And the venue also reports a long position of 3 ESH22 contracts without an opening price
      When the live node starts using <entrypoint>
      Then startup succeeds
      And the strategy observes a long position of 5 ESZ21 contracts at 4500.25 USD when it starts
      And the portfolio exposes 5 ESZ21 contracts when the strategy starts
      And the actor and strategy each start once
      And the venue receives a startup mass-status request

      Examples:
        | entrypoint |
        | start      |
        | run        |

  Rule: Startup continues when mass status is unavailable
    Unavailable mass status does not establish that the venue is flat.

    @unavailable
    Scenario Outline: Continue when the venue cannot provide mass status using <entrypoint>
      Given the venue cannot provide mass status
      When the live node starts using <entrypoint>
      Then startup succeeds
      And the actor and strategy each start once
      And the venue receives a startup mass-status request

      Examples:
        | entrypoint |
        | start      |
        | run        |

  Rule: A failed or timed-out mass-status request blocks startup

    @request_error
    Scenario Outline: Block startup when the mass-status request fails using <entrypoint>
      Given the venue returns an error for mass-status requests
      When the live node starts using <entrypoint>
      Then startup fails because the mass-status request failed
      And no actor or strategy starts
      And the venue receives a startup mass-status request
      And the execution client is disconnected

      Examples:
        | entrypoint |
        | start      |
        | run        |

    @timeout
    Scenario Outline: Block startup when the reconciliation deadline expires using <entrypoint>
      Given the venue never completes the mass-status request
      When the live node starts using <entrypoint>
      Then startup fails because the reconciliation deadline expires
      And no actor or strategy starts
      And the venue receives a startup mass-status request
      And the execution client is disconnected

      Examples:
        | entrypoint |
        | start      |
        | run        |

  Rule: Disabling startup reconciliation skips startup mass-status requests

    @disabled
    Scenario Outline: Skip startup mass-status requests when reconciliation is disabled using <entrypoint>
      Given startup reconciliation is disabled
      And the venue returns an error for mass-status requests
      When the live node starts using <entrypoint>
      Then startup succeeds
      And the actor and strategy each start once
      And the venue receives no startup mass-status request

      Examples:
        | entrypoint |
        | start      |
        | run        |

  Rule: An explicit flat position report and a missing report have different meanings
    An explicit flat report closes retained exposure before trading starts
    when the retaining strategy claims external orders for the instrument.
    Without position reports or executions, startup preserves retained exposure.

    @retained_position @explicit_flat
    Scenario Outline: Close retained exposure from an explicit flat report using <entrypoint>
      Given the execution client uses NETTING accounting
      And a retained long position holds 1.000 ETH bought at 3000.00 USDT with a 0.60 USDT fee
      And the strategy claims external orders for the retained position's instrument
      And the venue reports an explicit flat position and no orders or fills
      When the live node starts using <entrypoint>
      Then startup succeeds
      And the retained position is flat with 0.000 ETH when the strategy starts
      And the portfolio exposes 0 ETH when the strategy starts
      And exactly one position is cached
      And the actor and strategy each start once
      And the venue receives a startup mass-status request

      Examples:
        | entrypoint |
        | start      |
        | run        |

    @retained_position @missing_report
    Scenario Outline: Preserve retained exposure when no position report arrives using <entrypoint>
      Given the execution client uses NETTING accounting
      And a retained long position holds 1.000 ETH bought at 3000.00 USDT with a 0.60 USDT fee
      And the strategy claims external orders for the retained position's instrument
      And the venue reports no orders, fills, or positions
      When the live node starts using <entrypoint>
      Then startup succeeds
      And the retained position is long with 1.000 ETH when the strategy starts
      And the retained position remains long 1.000 ETH with realized profit of -0.60 USDT and total fees of 0.60 USDT when the strategy starts
      And the portfolio exposes 1.000 ETH when the strategy starts
      And exactly one position is cached
      And the actor and strategy each start once
      And the venue receives a startup mass-status request

      Examples:
        | entrypoint |
        | start      |
        | run        |

  Rule: Startup recovers claimed venue orders when unclaimed external orders are filtered

    Background:
      Given the cache has AAPL equity on NASDAQ and a USD cash account
      And the execution client uses NETTING accounting
      And unclaimed external orders are filtered
      And the venue reports an accepted buy limit order XNAS-42 for 25 AAPL shares at 182.35 USD

    @external_orders
    Scenario Outline: Recover a claimed open order using <entrypoint>
      Given the strategy claims external orders for AAPL on NASDAQ
      When the live node starts using <entrypoint>
      Then startup succeeds
      And the strategy owns one open AAPL buy limit order XNAS-42 for 25 shares at 182.35 USD when it starts
      And the venue receives a startup mass-status request

      Examples:
        | entrypoint |
        | start      |
        | run        |

    @external_orders
    Scenario Outline: Filter an unclaimed open order using <entrypoint>
      Given no strategy claims external orders for AAPL on NASDAQ
      When the live node starts using <entrypoint>
      Then startup succeeds
      And the strategy observes an empty order cache when it starts
      And the venue receives a startup mass-status request

      Examples:
        | entrypoint |
        | start      |
        | run        |

  Rule: Closing executions recover retained exposure when the venue cannot report positions
    A close executed while the node is offline must update the retained position,
    its realized profit, and its fees before the strategy starts.
    A cached order already belongs to the strategy. An externally placed order
    must be explicitly claimed by that strategy.
    A missing position report does not by itself prove that the position is flat.

    @retained_close
    Scenario Outline: Recover an offline close from a <source> order
      Given the execution client uses NETTING accounting
      And a retained long position holds 1.000 ETH bought at 3000.00 USDT with a 0.60 USDT fee
      And the client cannot provide complete position coverage
      And the closing order is <source>
      And the bounded history starts after the opening trade
      And the venue reports a filled closing order and no position
      And the venue reports these closing executions
        | quantity | price   | fee  |
        | 0.400    | 3100.00 | 0.25 |
        | 0.600    | 3120.00 | 0.37 |
      When the live node starts using start
      Then startup succeeds
      And the closing order is FILLED
      And the retained position is flat with realized profit of 110.78 USDT and total fees of 1.22 USDT when the strategy starts
      And exactly one position is cached
      And the portfolio exposes 0 ETH when the strategy starts
      And the actor and strategy each start once

      Examples:
        | source  |
        | cached  |
        | claimed |

    @retained_close @restart_replay
    Scenario: Replay the same closing executions after restart
      Given the cache has the ESZ21 futures contract and a USD margin account
      And the execution client uses NETTING accounting
      And a retained long position holds 3 ESZ21 contracts bought at 4500.00 USD with a 6.75 USD fee
      And the client cannot provide complete position coverage
      And the closing order is cached
      And the bounded history starts after the opening trade
      And the venue reports a closing order filled at an average of 4502.25 USD and no position
      And the venue reports these closing executions
        | quantity | price   | fee  |
        | 1        | 4501.75 | 2.25 |
        | 2        | 4502.50 | 4.50 |
      When the live node starts and restarts using start, replaying the same reports against the recovered orders and position
      Then startup succeeds
      And the closing order is FILLED
      And the closing executions are recorded once on the order and position when the strategy starts
      And the retained position is flat with realized profit of 324.00 USD and total fees of 13.50 USD when the strategy starts
      And exactly one position is cached
      And the portfolio exposes 0 ESZ21 contracts when the strategy starts
      And the actor and strategy each start once
      And the venue receives a startup mass-status request

    @retained_close @incomplete
    Scenario: Incomplete history cannot establish a safe position reduction
      Given the execution client uses NETTING accounting
      And a retained long position holds 1.000 ETH bought at 3000.00 USDT with a 0.60 USDT fee
      And the client cannot provide complete position coverage
      And the closing order is cached
      And the bounded history starts after the opening trade
      And the venue reports a filled closing order and no position
      And the venue reports these closing executions
        | quantity | price   | fee  |
        | 0.400    | 3100.00 | 0.25 |
        | 0.600    | 3120.00 | 0.37 |
      But the venue marks the history incomplete
      When the live node starts using start
      Then startup succeeds
      And the closing order is FILLED
      And the retained position remains long 1.000 ETH with realized profit of -0.60 USDT and total fees of 0.60 USDT when the strategy starts
      And exactly one position is cached
      And the portfolio exposes 1.000 ETH when the strategy starts

    @retained_close @no_executions
    Scenario: A filled order report alone does not recover closing profit
      Given the execution client uses NETTING accounting
      And a retained long position holds 1.000 ETH bought at 3000.00 USDT with a 0.60 USDT fee
      And the client cannot provide complete position coverage
      And the closing order is cached
      And the bounded history starts after the opening trade
      And the venue reports a filled closing order and no position
      But the venue supplies no closing executions
      When the live node starts using start
      Then startup succeeds
      And the closing order is FILLED
      And the retained position remains long 1.000 ETH with realized profit of -0.60 USDT and total fees of 0.60 USDT when the strategy starts
      And exactly one position is cached
      And the portfolio exposes 1.000 ETH when the strategy starts
