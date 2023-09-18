use std::{collections::HashMap, str::FromStr};

use ethers::types::{Address, Opcode, H256, U256};
use rundler_types::{Entity, EntityType};
use serde::Deserialize;
use serde_with::{serde_as, DisplayFromStr};

use crate::common::simulation::SimulationViolation;

/// The entity allowed by an allowlist entry.
#[derive(Debug, Copy, Clone)]
pub enum AllowEntity {
    /// Any entity is allowed.
    Any,
    /// Entity of a specific type is allowed.
    Type(EntityType),
    /// Entity at a specific address is allowed.
    Address(Address),
}

impl FromStr for AllowEntity {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let out = match s {
            "*" => AllowEntity::Any,
            "account" => AllowEntity::Type(EntityType::Account),
            "paymaster" => AllowEntity::Type(EntityType::Paymaster),
            "aggregator" => AllowEntity::Type(EntityType::Aggregator),
            "factory" => AllowEntity::Type(EntityType::Factory),
            _ => AllowEntity::Address(s.parse()?),
        };
        Ok(out)
    }
}

impl AllowEntity {
    fn is_allowed(&self, entity: &Entity) -> bool {
        match self {
            AllowEntity::Any => true,
            AllowEntity::Type(kind) => entity.kind == *kind,
            AllowEntity::Address(address) => entity.address == *address,
        }
    }
}

/// An allowlist rule.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "rule", rename_all = "camelCase")]
pub enum AllowRule {
    /// Allowlist a forbidden opcode on a contract.
    ForbiddenOpcode { contract: Address, opcode: Opcode },
    /// Allowlist a forbidden precompile by its address on a contract
    ForbiddenPrecompile {
        contract: Address,
        precompile: Address,
    },
    /// Allowlist an invalid storage access by its address/slot
    InvalidStorageAccess { contract: Address, slot: U256 },
    /// Allowlist a call with value
    CallWithValue,
    /// Allowlist a not staked violation
    NotStaked,
}

/// An allowlist entry
#[serde_as]
#[derive(Debug, Clone, Deserialize)]
pub struct AllowlistEntry {
    /// The entity allowed by this entry.
    #[serde_as(as = "DisplayFromStr")]
    entity: AllowEntity,
    /// The rule allowed by this entry.
    #[serde(flatten)]
    rule: AllowRule,
}

impl AllowlistEntry {
    pub fn new(entity: AllowEntity, rule: AllowRule) -> Self {
        Self { entity, rule }
    }

    /// Check if the allowlist entry allows the given violation.
    pub fn is_allowed(&self, violation: &SimulationViolation) -> bool {
        match &self.rule {
            AllowRule::ForbiddenOpcode { contract, opcode } => {
                if let SimulationViolation::UsedForbiddenOpcode(
                    violation_entity,
                    violation_contract,
                    violation_opcode,
                ) = violation
                {
                    self.entity.is_allowed(violation_entity)
                        && contract == violation_contract
                        && *opcode == violation_opcode.0
                } else {
                    false
                }
            }
            AllowRule::ForbiddenPrecompile {
                contract,
                precompile,
            } => {
                if let SimulationViolation::UsedForbiddenPrecompile(
                    violation_entity,
                    violation_contract,
                    violation_address,
                ) = violation
                {
                    self.entity.is_allowed(violation_entity)
                        && contract == violation_contract
                        && precompile == violation_address
                } else {
                    false
                }
            }
            AllowRule::InvalidStorageAccess { contract, slot } => {
                if let SimulationViolation::InvalidStorageAccess(violation_entity, violation_slot) =
                    violation
                {
                    self.entity.is_allowed(violation_entity)
                        && *contract == violation_slot.address
                        && *slot == violation_slot.slot
                } else {
                    false
                }
            }
            AllowRule::CallWithValue => {
                if let SimulationViolation::CallHadValue(violation_entity) = violation {
                    self.entity.is_allowed(violation_entity)
                } else {
                    false
                }
            }
            AllowRule::NotStaked => {
                if let SimulationViolation::NotStaked(violation_entity, _, _) = violation {
                    self.entity.is_allowed(violation_entity)
                } else {
                    false
                }
            }
        }
    }
}

/// A mempool configuration.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct MempoolConfig {
    /// Allowlist to match violations against.
    allowlist: Vec<AllowlistEntry>,
}

/// Return value for matching mempools
#[derive(Debug, PartialEq, Eq)]
pub enum MempoolMatchResult {
    /// One or more matched mempools by ID
    Matches(Vec<H256>),
    /// No mempools matched, with the index of the first violation that didn't match
    NoMatch(usize),
}

/// Match mempools based on a list of violations. Operations are matched to each of the
/// mempools in which all of their violations are allowlisted. If zero violations,
/// an operation will match all mempools.
pub fn match_mempools(
    mempools: &HashMap<H256, MempoolConfig>,
    violations: &[SimulationViolation],
) -> MempoolMatchResult {
    let mut candidate_pools: Vec<H256> = mempools.keys().cloned().collect();
    for (i, violation) in violations.iter().enumerate() {
        candidate_pools.retain(|p| {
            mempools[p]
                .allowlist
                .iter()
                .any(|r| r.is_allowed(violation))
        });
        if candidate_pools.is_empty() {
            return MempoolMatchResult::NoMatch(i);
        }
    }
    MempoolMatchResult::Matches(candidate_pools)
}

#[cfg(test)]
mod tests {
    use ethers::types::U256;

    use super::*;
    use crate::common::simulation::{StorageSlot, ViolationOpCode};

    #[test]
    fn test_allow_entity_any() {
        let allow = AllowEntity::Any;

        let account_entity = Entity {
            kind: EntityType::Account,
            address: Address::zero(),
        };
        assert!(allow.is_allowed(&account_entity));

        let paymaster_entity = Entity {
            kind: EntityType::Paymaster,
            address: Address::random(),
        };
        assert!(allow.is_allowed(&paymaster_entity));

        let factory_entity = Entity {
            kind: EntityType::Factory,
            address: Address::random(),
        };
        assert!(allow.is_allowed(&factory_entity));
    }

    #[test]
    fn test_allow_entity_type() {
        let allow = AllowEntity::Type(EntityType::Paymaster);

        let account_entity = Entity {
            kind: EntityType::Account,
            address: Address::zero(),
        };
        assert!(!allow.is_allowed(&account_entity));

        let paymaster_entity = Entity {
            kind: EntityType::Paymaster,
            address: Address::random(),
        };
        assert!(allow.is_allowed(&paymaster_entity));

        let factory_entity = Entity {
            kind: EntityType::Factory,
            address: Address::random(),
        };
        assert!(!allow.is_allowed(&factory_entity));
    }

    #[test]
    fn test_allow_entity_address() {
        let addr = Address::random();
        let allow = AllowEntity::Address(addr);

        let account_entity = Entity {
            kind: EntityType::Account,
            address: addr,
        };
        assert!(allow.is_allowed(&account_entity));

        let paymaster_entity = Entity {
            kind: EntityType::Paymaster,
            address: addr,
        };
        assert!(allow.is_allowed(&paymaster_entity));

        let factory_entity = Entity {
            kind: EntityType::Factory,
            address: Address::random(),
        };
        assert!(!allow.is_allowed(&factory_entity));
    }

    #[test]
    fn test_allowlist_entry_forbidden_opcode() {
        let contract = Address::random();
        let entry = AllowlistEntry::new(
            AllowEntity::Type(EntityType::Account),
            AllowRule::ForbiddenOpcode {
                contract,
                opcode: Opcode::GAS,
            },
        );

        let violation = SimulationViolation::UsedForbiddenOpcode(
            Entity {
                kind: EntityType::Account,
                address: Address::zero(),
            },
            contract,
            ViolationOpCode(Opcode::GAS),
        );
        assert!(entry.is_allowed(&violation));

        let violation = SimulationViolation::UsedForbiddenOpcode(
            Entity {
                kind: EntityType::Account,
                address: Address::zero(),
            },
            contract,
            ViolationOpCode(Opcode::BLOCKHASH),
        );
        assert!(!entry.is_allowed(&violation));
    }

    #[test]
    fn test_allowlist_entry_forbidden_precompile() {
        let addr = Address::random();
        let contract = Address::random();
        let precompile = Address::random();
        let entry = AllowlistEntry::new(
            AllowEntity::Address(addr),
            AllowRule::ForbiddenPrecompile {
                contract,
                precompile,
            },
        );

        let violation = SimulationViolation::UsedForbiddenPrecompile(
            Entity {
                kind: EntityType::Account,
                address: addr,
            },
            contract,
            precompile,
        );
        assert!(entry.is_allowed(&violation));

        let violation = SimulationViolation::UsedForbiddenPrecompile(
            Entity {
                kind: EntityType::Account,
                address: Address::random(),
            },
            contract,
            precompile,
        );
        assert!(!entry.is_allowed(&violation));

        let violation = SimulationViolation::UsedForbiddenPrecompile(
            Entity {
                kind: EntityType::Account,
                address: addr,
            },
            contract,
            Address::random(),
        );
        assert!(!entry.is_allowed(&violation));
    }

    #[test]
    fn test_allowlist_entry_invalid_storage_access() {
        let entity_addr = Address::random();
        let slot_addr = Address::random();
        let slot_slot = U256::from(1234567890);
        let slot = StorageSlot {
            address: slot_addr,
            slot: slot_slot,
        };
        let entry = AllowlistEntry::new(
            AllowEntity::Address(entity_addr),
            AllowRule::InvalidStorageAccess {
                contract: slot_addr,
                slot: slot_slot,
            },
        );

        let violation = SimulationViolation::InvalidStorageAccess(
            Entity {
                kind: EntityType::Account,
                address: entity_addr,
            },
            slot,
        );
        assert!(entry.is_allowed(&violation));

        let violation = SimulationViolation::InvalidStorageAccess(
            Entity {
                kind: EntityType::Account,
                address: Address::random(),
            },
            slot,
        );
        assert!(!entry.is_allowed(&violation));

        let violation = SimulationViolation::InvalidStorageAccess(
            Entity {
                kind: EntityType::Account,
                address: entity_addr,
            },
            StorageSlot {
                address: slot_addr,
                slot: U256::from(0),
            },
        );
        assert!(!entry.is_allowed(&violation));

        let violation = SimulationViolation::InvalidStorageAccess(
            Entity {
                kind: EntityType::Account,
                address: entity_addr,
            },
            StorageSlot {
                address: Address::random(),
                slot: U256::from(1234567890),
            },
        );
        assert!(!entry.is_allowed(&violation));
    }

    #[test]
    fn test_allowlist_call_with_value() {
        let entity_addr = Address::random();
        let entry =
            AllowlistEntry::new(AllowEntity::Address(entity_addr), AllowRule::CallWithValue);

        let violation = SimulationViolation::CallHadValue(Entity {
            kind: EntityType::Account,
            address: entity_addr,
        });
        assert!(entry.is_allowed(&violation));

        let violation = SimulationViolation::CallHadValue(Entity {
            kind: EntityType::Account,
            address: Address::random(),
        });
        assert!(!entry.is_allowed(&violation));
    }

    #[test]
    fn test_allowlist_not_staked() {
        let entity_addr = Address::random();
        let entry = AllowlistEntry::new(AllowEntity::Address(entity_addr), AllowRule::NotStaked);

        let violation = SimulationViolation::NotStaked(
            Entity {
                kind: EntityType::Account,
                address: entity_addr,
            },
            U256::from(0),
            U256::from(0),
        );
        assert!(entry.is_allowed(&violation));

        let violation = SimulationViolation::NotStaked(
            Entity {
                kind: EntityType::Account,
                address: Address::random(),
            },
            U256::from(0),
            U256::from(0),
        );
        assert!(!entry.is_allowed(&violation));
    }

    #[test]
    fn test_match_none() {
        let contract = Address::random();
        let mempools = HashMap::from([
            (H256::random(), MempoolConfig::default()),
            (
                H256::random(),
                MempoolConfig {
                    allowlist: vec![AllowlistEntry::new(
                        AllowEntity::Type(EntityType::Account),
                        AllowRule::ForbiddenOpcode {
                            contract,
                            opcode: Opcode::GAS,
                        },
                    )],
                },
            ),
        ]);
        let violation = SimulationViolation::UsedForbiddenOpcode(
            Entity {
                kind: EntityType::Account,
                address: Address::random(),
            },
            contract,
            ViolationOpCode(Opcode::BLOCKHASH),
        );
        assert_eq!(
            match_mempools(&mempools, &[violation]),
            MempoolMatchResult::NoMatch(0)
        );
    }

    #[test]
    fn test_match_none_second() {
        let contract = Address::random();
        let mempools = HashMap::from([
            (H256::random(), MempoolConfig::default()),
            (
                H256::random(),
                MempoolConfig {
                    allowlist: vec![AllowlistEntry::new(
                        AllowEntity::Type(EntityType::Account),
                        AllowRule::ForbiddenOpcode {
                            contract,
                            opcode: Opcode::GAS,
                        },
                    )],
                },
            ),
        ]);
        let violations = [
            SimulationViolation::UsedForbiddenOpcode(
                Entity {
                    kind: EntityType::Account,
                    address: Address::random(),
                },
                contract,
                ViolationOpCode(Opcode::GAS),
            ),
            SimulationViolation::UsedForbiddenOpcode(
                Entity {
                    kind: EntityType::Account,
                    address: Address::random(),
                },
                contract,
                ViolationOpCode(Opcode::BLOCKHASH),
            ),
        ];
        assert_eq!(
            match_mempools(&mempools, &violations),
            MempoolMatchResult::NoMatch(1)
        );
    }

    #[test]
    fn test_match_one() {
        let mempool0 = H256::random();
        let mempool1 = H256::random();
        let contract = Address::random();
        let mempools = HashMap::from([
            (mempool0, MempoolConfig::default()),
            (
                mempool1,
                MempoolConfig {
                    allowlist: vec![AllowlistEntry::new(
                        AllowEntity::Type(EntityType::Account),
                        AllowRule::ForbiddenOpcode {
                            contract,
                            opcode: Opcode::GAS,
                        },
                    )],
                },
            ),
        ]);
        let violations = [SimulationViolation::UsedForbiddenOpcode(
            Entity {
                kind: EntityType::Account,
                address: Address::random(),
            },
            contract,
            ViolationOpCode(Opcode::GAS),
        )];
        assert_eq!(
            match_mempools(&mempools, &violations),
            MempoolMatchResult::Matches(vec![mempool1])
        );
    }

    #[test]
    fn test_match_multiple() {
        let mempool0 = H256::random();
        let mempool1 = H256::random();
        let mempool2 = H256::random();
        let contract = Address::random();
        let mempools = HashMap::from([
            (mempool0, MempoolConfig::default()),
            (
                mempool1,
                MempoolConfig {
                    allowlist: vec![
                        AllowlistEntry::new(
                            AllowEntity::Type(EntityType::Account),
                            AllowRule::ForbiddenOpcode {
                                contract,
                                opcode: Opcode::GAS,
                            },
                        ),
                        AllowlistEntry::new(
                            AllowEntity::Type(EntityType::Account),
                            AllowRule::ForbiddenOpcode {
                                contract,
                                opcode: Opcode::BASEFEE,
                            },
                        ),
                    ],
                },
            ),
            (
                mempool2,
                MempoolConfig {
                    allowlist: vec![
                        AllowlistEntry::new(
                            AllowEntity::Type(EntityType::Account),
                            AllowRule::ForbiddenOpcode {
                                contract,
                                opcode: Opcode::BLOCKHASH,
                            },
                        ),
                        AllowlistEntry::new(
                            AllowEntity::Type(EntityType::Account),
                            AllowRule::ForbiddenOpcode {
                                contract,
                                opcode: Opcode::GAS,
                            },
                        ),
                        AllowlistEntry::new(
                            AllowEntity::Type(EntityType::Account),
                            AllowRule::ForbiddenOpcode {
                                contract,
                                opcode: Opcode::BASEFEE,
                            },
                        ),
                    ],
                },
            ),
        ]);
        let violations = [
            SimulationViolation::UsedForbiddenOpcode(
                Entity {
                    kind: EntityType::Account,
                    address: Address::random(),
                },
                contract,
                ViolationOpCode(Opcode::GAS),
            ),
            SimulationViolation::UsedForbiddenOpcode(
                Entity {
                    kind: EntityType::Account,
                    address: Address::random(),
                },
                contract,
                ViolationOpCode(Opcode::BASEFEE),
            ),
        ];

        match match_mempools(&mempools, &violations) {
            MempoolMatchResult::Matches(mempools) => {
                assert_eq!(mempools.len(), 2);
                assert!(mempools.contains(&mempool1));
                assert!(mempools.contains(&mempool2));
            }
            _ => panic!("Expected matches"),
        }
    }
}