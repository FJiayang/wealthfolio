//! MCP-only business tools. Domain services own validation and persistence.
use super::business_support::*;
use crate::{
    AgentEnvironment, AgentScope, AgentTool, AgentToolAccess, AgentToolError, AgentToolResult,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PrepareRetirementSimulationInputArgs {
    pub goal_id: String,
}

business_tool!(PrepareRetirementSimulationInput, PrepareRetirementSimulationInputArgs, "prepare_retirement_simulation_input",
    "Read the saved goal plan and current valuations, resolving its backend-owned retirement plan, currentPortfolio and plannerMode for calculation tools.",
    json!({"type":"object","properties":{"goalId":{"type":"string"}},"required":["goalId"]}), [AgentScope::FinancialPlanningRead, AgentScope::HoldingsRead], Read, |env, args| {
        let valuations = valuation_map(&env).await?;
        let input = env.goal_service().prepare_retirement_simulation_input(&args.goal_id, &valuations).await?;
        output(json!({"plan": input.plan, "currentPortfolio": input.current_portfolio, "plannerMode": input.planner_mode}))
    });

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CalculateRetirementProjectionArgs {
    pub plan: wealthfolio_core::planning::retirement::RetirementPlan,
    pub current_portfolio: f64,
    #[serde(default)]
    pub planner_mode: Option<wealthfolio_core::planning::retirement::RetirementTimingMode>,
}

business_tool!(CalculateRetirementProjection, CalculateRetirementProjectionArgs, "calculate_retirement_projection",
    "Calculate a retirement scenario from supplied plan data without modifying goals or holdings. For a saved goal first call prepare_retirement_simulation_input; Monte Carlo uses the frontend limit of 1 to 500000 simulations.",
    json!({"type":"object","properties":{"plan":retirement_plan_schema(),"currentPortfolio":{"type":"number"},"plannerMode":{"type":"string","enum":["fire","traditional"]}},"required":["plan","currentPortfolio"]}), [AgentScope::FinancialPlanningRead], Read, |env, args| {
        let as_of = wealthfolio_core::utils::time_utils::user_today(wealthfolio_core::utils::time_utils::parse_user_timezone_or_default(&timezone(&env)?));
        let mut plan = args.plan;
        wealthfolio_core::planning::retirement::normalize_retirement_plan_ages(&mut plan, as_of);
        wealthfolio_core::goals::validate_retirement_plan(&plan)?;
        if !args.current_portfolio.is_finite() || args.current_portfolio < 0.0 { return Err(AgentToolError::InvalidInput("currentPortfolio must be non-negative".into())); }
        let current_portfolio = args.current_portfolio;
        let mode = args.planner_mode.unwrap_or_default();
        output(wealthfolio_core::portfolio::fire::project_retirement_with_mode(&plan, current_portfolio, mode, as_of))
    });

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RunRetirementMonteCarloArgs {
    pub plan: wealthfolio_core::planning::retirement::RetirementPlan,
    pub current_portfolio: f64,
    #[serde(default)]
    pub planner_mode: Option<wealthfolio_core::planning::retirement::RetirementTimingMode>,
    #[serde(default)]
    pub n_sims: Option<u32>,
    #[serde(default)]
    pub seed: Option<u64>,
}

business_tool!(RunRetirementMonteCarlo, RunRetirementMonteCarloArgs, "run_retirement_monte_carlo",
    "Calculate a retirement scenario from supplied plan data without modifying goals or holdings. For a saved goal first call prepare_retirement_simulation_input; Monte Carlo uses the frontend limit of 1 to 500000 simulations.",
    json!({"type":"object","properties":{"plan":retirement_plan_schema(),"currentPortfolio":{"type":"number"},"plannerMode":{"type":"string","enum":["fire","traditional"]},"nSims":{"type":"integer"},"seed":{"type":"integer"}},"required":["plan","currentPortfolio"]}), [AgentScope::FinancialPlanningRead], Read, |env, args| {
        let as_of = wealthfolio_core::utils::time_utils::user_today(wealthfolio_core::utils::time_utils::parse_user_timezone_or_default(&timezone(&env)?));
        let mut plan = args.plan;
        wealthfolio_core::planning::retirement::normalize_retirement_plan_ages(&mut plan, as_of);
        wealthfolio_core::goals::validate_retirement_plan(&plan)?;
        if !args.current_portfolio.is_finite() || args.current_portfolio < 0.0 { return Err(AgentToolError::InvalidInput("currentPortfolio must be non-negative".into())); }
        let current_portfolio = args.current_portfolio;
        let mode = args.planner_mode.unwrap_or_default();
        output(tokio::task::spawn_blocking(move || wealthfolio_core::portfolio::fire::run_monte_carlo_with_mode_and_seed(&plan, current_portfolio, args.n_sims.unwrap_or(10_000).clamp(1, 500_000), mode, args.seed)).await.map_err(|_| AgentToolError::ExecutionFailed("Retirement calculation task failed".into()))?)
    });

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RunRetirementStressTestsArgs {
    pub plan: wealthfolio_core::planning::retirement::RetirementPlan,
    pub current_portfolio: f64,
    #[serde(default)]
    pub planner_mode: Option<wealthfolio_core::planning::retirement::RetirementTimingMode>,
}

business_tool!(RunRetirementStressTests, RunRetirementStressTestsArgs, "run_retirement_stress_tests",
    "Calculate a retirement scenario from supplied plan data without modifying goals or holdings. For a saved goal first call prepare_retirement_simulation_input; Monte Carlo uses the frontend limit of 1 to 500000 simulations.",
    json!({"type":"object","properties":{"plan":retirement_plan_schema(),"currentPortfolio":{"type":"number"},"plannerMode":{"type":"string","enum":["fire","traditional"]}},"required":["plan","currentPortfolio"]}), [AgentScope::FinancialPlanningRead], Read, |env, args| {
        let as_of = wealthfolio_core::utils::time_utils::user_today(wealthfolio_core::utils::time_utils::parse_user_timezone_or_default(&timezone(&env)?));
        let mut plan = args.plan;
        wealthfolio_core::planning::retirement::normalize_retirement_plan_ages(&mut plan, as_of);
        wealthfolio_core::goals::validate_retirement_plan(&plan)?;
        if !args.current_portfolio.is_finite() || args.current_portfolio < 0.0 { return Err(AgentToolError::InvalidInput("currentPortfolio must be non-negative".into())); }
        let current_portfolio = args.current_portfolio;
        let mode = args.planner_mode.unwrap_or_default();
        output(tokio::task::spawn_blocking(move || wealthfolio_core::portfolio::fire::run_stress_tests_with_mode(&plan, current_portfolio, mode, as_of)).await.map_err(|_| AgentToolError::ExecutionFailed("Retirement calculation task failed".into()))?)
    });

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RunRetirementScenarioAnalysisArgs {
    pub plan: wealthfolio_core::planning::retirement::RetirementPlan,
    pub current_portfolio: f64,
    #[serde(default)]
    pub planner_mode: Option<wealthfolio_core::planning::retirement::RetirementTimingMode>,
}

business_tool!(RunRetirementScenarioAnalysis, RunRetirementScenarioAnalysisArgs, "run_retirement_scenario_analysis",
    "Calculate a retirement scenario from supplied plan data without modifying goals or holdings. For a saved goal first call prepare_retirement_simulation_input; Monte Carlo uses the frontend limit of 1 to 500000 simulations.",
    json!({"type":"object","properties":{"plan":retirement_plan_schema(),"currentPortfolio":{"type":"number"},"plannerMode":{"type":"string","enum":["fire","traditional"]}},"required":["plan","currentPortfolio"]}), [AgentScope::FinancialPlanningRead], Read, |env, args| {
        let as_of = wealthfolio_core::utils::time_utils::user_today(wealthfolio_core::utils::time_utils::parse_user_timezone_or_default(&timezone(&env)?));
        let mut plan = args.plan;
        wealthfolio_core::planning::retirement::normalize_retirement_plan_ages(&mut plan, as_of);
        wealthfolio_core::goals::validate_retirement_plan(&plan)?;
        if !args.current_portfolio.is_finite() || args.current_portfolio < 0.0 { return Err(AgentToolError::InvalidInput("currentPortfolio must be non-negative".into())); }
        let current_portfolio = args.current_portfolio;
        let mode = args.planner_mode.unwrap_or_default();
        output(tokio::task::spawn_blocking(move || wealthfolio_core::portfolio::fire::run_scenario_analysis_with_mode(&plan, current_portfolio, mode, as_of)).await.map_err(|_| AgentToolError::ExecutionFailed("Retirement calculation task failed".into()))?)
    });

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RunRetirementDecisionSensitivityMapArgs {
    pub plan: wealthfolio_core::planning::retirement::RetirementPlan,
    pub current_portfolio: f64,
    #[serde(default)]
    pub planner_mode: Option<wealthfolio_core::planning::retirement::RetirementTimingMode>,
    pub map: wealthfolio_core::portfolio::fire::DecisionSensitivityMap,
}

business_tool!(RunRetirementDecisionSensitivityMap, RunRetirementDecisionSensitivityMapArgs, "run_retirement_decision_sensitivity_map",
    "Calculate a retirement scenario from supplied plan data without modifying goals or holdings. For a saved goal first call prepare_retirement_simulation_input; Monte Carlo uses the frontend limit of 1 to 500000 simulations.",
    json!({"type":"object","properties":{"plan":retirement_plan_schema(),"currentPortfolio":{"type":"number"},"plannerMode":{"type":"string","enum":["fire","traditional"]},"map":{"type":"string","enum":["contribution-return","retirement-age-spending"]}},"required":["plan","currentPortfolio","map"]}), [AgentScope::FinancialPlanningRead], Read, |env, args| {
        let as_of = wealthfolio_core::utils::time_utils::user_today(wealthfolio_core::utils::time_utils::parse_user_timezone_or_default(&timezone(&env)?));
        let mut plan = args.plan;
        wealthfolio_core::planning::retirement::normalize_retirement_plan_ages(&mut plan, as_of);
        wealthfolio_core::goals::validate_retirement_plan(&plan)?;
        if !args.current_portfolio.is_finite() || args.current_portfolio < 0.0 { return Err(AgentToolError::InvalidInput("currentPortfolio must be non-negative".into())); }
        let current_portfolio = args.current_portfolio;
        let mode = args.planner_mode.unwrap_or_default();
        output(tokio::task::spawn_blocking(move || wealthfolio_core::portfolio::fire::run_decision_sensitivity_matrix_with_mode(&plan, current_portfolio, mode, args.map, as_of)).await.map_err(|_| AgentToolError::ExecutionFailed("Retirement calculation task failed".into()))?)
    });

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RunRetirementSorrArgs {
    pub plan: wealthfolio_core::planning::retirement::RetirementPlan,
    pub portfolio_at_fire: f64,
    pub retirement_start_age: u32,
}

business_tool!(RunRetirementSorr, RunRetirementSorrArgs, "run_retirement_sorr",
    "Calculate retirement return-sequence scenarios from supplied plan data. Does not modify any records.",
    json!({"type":"object","properties":{"plan":retirement_plan_schema(),"portfolioAtFire":{"type":"number"},"retirementStartAge":{"type":"integer"}},"required":["plan","portfolioAtFire","retirementStartAge"]}), [AgentScope::FinancialPlanningRead], Read, |env, args| {
        let as_of = wealthfolio_core::utils::time_utils::user_today(wealthfolio_core::utils::time_utils::parse_user_timezone_or_default(&timezone(&env)?));
        let mut plan = args.plan;
        wealthfolio_core::planning::retirement::normalize_retirement_plan_ages(&mut plan, as_of);
        wealthfolio_core::goals::validate_retirement_plan(&plan)?;
        if !args.portfolio_at_fire.is_finite() || args.portfolio_at_fire < 0.0 { return Err(AgentToolError::InvalidInput("portfolioAtFire must be non-negative".into())); }
        output(tokio::task::spawn_blocking(move || wealthfolio_core::portfolio::fire::run_sorr(&plan, args.portfolio_at_fire, args.retirement_start_age)).await.map_err(|_| AgentToolError::ExecutionFailed("Retirement calculation task failed".into()))?)
    });

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PreviewSaveUpOverviewArgs {
    pub input: wealthfolio_core::planning::SaveUpInput,
}

business_tool!(PreviewSaveUpOverview, PreviewSaveUpOverviewArgs, "preview_save_up_overview",
    "Preview a saving goal from supplied values using the current application date. Does not save a goal.",
    json!({"type":"object","properties":{"input":{"type":"object","properties":{"currentValue":{"type":"number"},"targetAmount":{"type":"number"},"targetDate":{"type":"string"},"monthlyContribution":{"type":"number"},"expectedAnnualReturn":{"type":"number"}},"required":["currentValue","targetAmount","monthlyContribution","expectedAnnualReturn"]}},"required":["input"]}), [AgentScope::FinancialPlanningRead], Read, |env, args| {
        let as_of = wealthfolio_core::utils::time_utils::user_today(wealthfolio_core::utils::time_utils::parse_user_timezone_or_default(&timezone(&env)?));
        wealthfolio_core::planning::validate_save_up_input(&args.input, as_of)?;
        output(wealthfolio_core::planning::compute_save_up_overview(&args.input, as_of))
    });

pub fn tools() -> Vec<Arc<dyn AgentTool>> {
    vec![
        Arc::new(PrepareRetirementSimulationInput),
        Arc::new(CalculateRetirementProjection),
        Arc::new(RunRetirementMonteCarlo),
        Arc::new(RunRetirementStressTests),
        Arc::new(RunRetirementScenarioAnalysis),
        Arc::new(RunRetirementDecisionSensitivityMap),
        Arc::new(RunRetirementSorr),
        Arc::new(PreviewSaveUpOverview),
    ]
}

fn retirement_plan_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "version": {
                "type": "string"
            },
            "personal": {
                "type": "object",
                "properties": {
                    "birthYearMonth": {
                        "type": "string"
                    },
                    "currentAge": {
                        "type": "integer"
                    },
                    "targetRetirementAge": {
                        "type": "integer"
                    },
                    "planningHorizonAge": {
                        "type": "integer"
                    },
                    "currentAnnualSalary": {
                        "type": "number"
                    },
                    "salaryGrowthRate": {
                        "type": "number"
                    }
                },
                "required": [
                    "currentAge",
                    "targetRetirementAge",
                    "planningHorizonAge"
                ]
            },
            "expenses": {
                "type": "object",
                "properties": {
                    "items": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "id": {
                                    "type": "string"
                                },
                                "label": {
                                    "type": "string"
                                },
                                "monthlyAmount": {
                                    "type": "number"
                                },
                                "inflationRate": {
                                    "type": "number"
                                },
                                "startAge": {
                                    "type": "integer"
                                },
                                "endAge": {
                                    "type": "integer"
                                },
                                "essential": {
                                    "type": "boolean"
                                }
                            },
                            "required": [
                                "monthlyAmount"
                            ]
                        },
                        "maxItems": 100
                    }
                },
                "required": []
            },
            "incomeStreams": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "id": {
                            "type": "string"
                        },
                        "label": {
                            "type": "string"
                        },
                        "linkedAccountId": {
                            "type": "string"
                        },
                        "startAge": {
                            "type": "integer"
                        },
                        "adjustForInflation": {
                            "type": "boolean"
                        },
                        "annualGrowthRate": {
                            "type": "number"
                        },
                        "monthlyAmount": {
                            "type": "number"
                        },
                        "currentValue": {
                            "type": "number"
                        },
                        "monthlyContribution": {
                            "type": "number"
                        },
                        "accumulationReturn": {
                            "type": "number"
                        },
                        "payoutRate": {
                            "type": "number"
                        },
                        "postPayoutReturn": {
                            "type": "number"
                        },
                        "streamType": {
                            "type": "string",
                            "enum": [
                                "db",
                                "dc"
                            ]
                        },
                        "payoutMode": {
                            "type": "string",
                            "enum": [
                                "annuity",
                                "drawdown"
                            ]
                        }
                    },
                    "required": [
                        "id",
                        "label",
                        "streamType",
                        "startAge",
                        "adjustForInflation"
                    ]
                }
            },
            "investment": {
                "type": "object",
                "properties": {
                    "preRetirementAnnualReturn": {
                        "type": "number"
                    },
                    "retirementAnnualReturn": {
                        "type": "number"
                    },
                    "annualInvestmentFeeRate": {
                        "type": "number"
                    },
                    "annualVolatility": {
                        "type": "number"
                    },
                    "inflationRate": {
                        "type": "number"
                    },
                    "monthlyContribution": {
                        "type": "number"
                    },
                    "contributionGrowthRate": {
                        "type": "number"
                    },
                    "glidePath": {
                        "type": "object",
                        "properties": {
                            "enabled": {
                                "type": "boolean"
                            },
                            "bondReturnRate": {
                                "type": "number"
                            },
                            "bondAllocationAtFire": {
                                "type": "number"
                            },
                            "bondAllocationAtHorizon": {
                                "type": "number"
                            }
                        },
                        "required": [
                            "enabled",
                            "bondReturnRate",
                            "bondAllocationAtFire",
                            "bondAllocationAtHorizon"
                        ]
                    }
                },
                "required": [
                    "inflationRate",
                    "monthlyContribution",
                    "contributionGrowthRate"
                ]
            },
            "tax": {
                "type": "object",
                "properties": {
                    "taxableWithdrawalRate": {
                        "type": "number"
                    },
                    "taxDeferredWithdrawalRate": {
                        "type": "number"
                    },
                    "taxFreeWithdrawalRate": {
                        "type": "number"
                    },
                    "earlyWithdrawalPenaltyRate": {
                        "type": "number"
                    },
                    "earlyWithdrawalPenaltyAge": {
                        "type": "integer"
                    },
                    "countryCode": {
                        "type": "string"
                    },
                    "withdrawalBuckets": {
                        "type": "object",
                        "properties": {
                            "taxable": {
                                "type": "number"
                            },
                            "taxDeferred": {
                                "type": "number"
                            },
                            "taxFree": {
                                "type": "number"
                            }
                        },
                        "required": [
                            "taxable",
                            "taxDeferred",
                            "taxFree"
                        ]
                    }
                },
                "required": [
                    "taxableWithdrawalRate",
                    "taxDeferredWithdrawalRate",
                    "taxFreeWithdrawalRate"
                ]
            },
            "currency": {
                "type": "string"
            }
        },
        "required": [
            "personal",
            "expenses",
            "incomeStreams",
            "investment",
            "currency"
        ]
    })
}
