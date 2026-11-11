import { createHash } from "node:crypto";
import { readFileSync, renameSync, writeFileSync } from "node:fs";
import path from "node:path";
import vm from "node:vm";

const FIXTURE_VERSION = 1;
const GENERATOR_REVISION = "pvpoke-node-harness-v1";
const TURN_DURATION_MS = 500;
const [fixturePath, outputPath = fixturePath] = process.argv.slice(2);

if (!fixturePath) {
  throw new Error("usage: generate-fixture.mjs <fixture.json> [output.json]");
}

const pvpokeRoot = process.env.PVPOKE_ROOT ?? "/pvpoke";
const dataPath = path.join(pvpokeRoot, "src/data/gamemaster.json");
const dataBytes = readFileSync(dataPath);
const fixture = JSON.parse(readFileSync(fixturePath, "utf8"));

function fail(message) {
  throw new Error(`${fixture.caseId ?? fixturePath}: ${message}`);
}

function gitBlobSha1(bytes) {
  const header = Buffer.from(`blob ${bytes.length}\0`);
  return createHash("sha1").update(header).update(bytes).digest("hex");
}

if (fixture.fixtureVersion !== FIXTURE_VERSION) {
  fail(`unsupported fixtureVersion ${fixture.fixtureVersion}`);
}
if (fixture.reference?.generatorRevision !== GENERATOR_REVISION) {
  fail(`reference.generatorRevision must be ${GENERATOR_REVISION}`);
}
if (fixture.reference.commit !== process.env.PVPOKE_COMMIT) {
  fail("fixture source commit does not match the mounted PvPoke checkout");
}
if (fixture.reference.gamemasterBlob !== gitBlobSha1(dataBytes)) {
  fail("fixture Game Master blob does not match the mounted PvPoke data");
}

const gmData = JSON.parse(dataBytes.toString("utf8"));
const pokemonById = new Map(gmData.pokemon.map((pokemon) => [pokemon.speciesId, pokemon]));
const moveById = new Map(gmData.moves.map((move) => [move.moveId, move]));
const gameMaster = {
  data: gmData,
  rankings: {},
  getPokemonById(id) {
    return structuredClone(pokemonById.get(id.replace("_xl", "")));
  },
  getMoveById(id) {
    return structuredClone(moveById.get(id));
  },
  getCupById() {
    return undefined;
  },
  generateFilteredPokemonList() {
    return [];
  },
};

const context = vm.createContext({
  console: { log() {}, warn() {}, error() {} },
  GameMaster: { getInstance: () => gameMaster },
  settings: { hardMovesetLinks: false },
  setTimeout,
  clearTimeout,
  setInterval,
  clearInterval,
});

const referenceScripts = [
  ["js/battle/DamageCalculator.js", "DamageCalculator"],
  ["js/battle/timeline/TimelineEvent.js", "TimelineEvent"],
  ["js/battle/timeline/TimelineAction.js", "TimelineAction"],
  ["js/battle/actions/ActionLogic.js", "ActionLogic"],
  ["js/pokemon/Pokemon.js", "Pokemon"],
  ["js/pokemon/Player.js", "Player"],
  ["js/battle/Battle.js", "Battle"],
];

for (const [relativePath, globalName] of referenceScripts) {
  const source = readFileSync(path.join(pvpokeRoot, "src", relativePath), "utf8");
  vm.runInContext(
    `${source}\nglobalThis.${globalName} = ${globalName};`,
    context,
    { filename: relativePath },
  );
}

const setup = fixture.input?.battleSetup;
if (!setup || !Array.isArray(setup.participants) || setup.participants.length !== 2) {
  fail("input.battleSetup must contain exactly two ordered participants");
}
if (setup.participants.some((participant) => participant.team.length !== 1)) {
  fail("this harness revision supports one active Pokémon per participant");
}
if (!Array.isArray(fixture.input.steps) || fixture.input.steps.length === 0) {
  fail("input.steps must contain explicit action requests");
}

const battle = new context.Battle();
battle.setCP(setup.cpLimit ?? 1500);
battle.setLevelCap(setup.levelCap ?? 50);

const players = [];
const activePokemon = [];
const participantsById = new Map();
for (const [participantIndex, participant] of setup.participants.entries()) {
  if (participantsById.has(participant.participantId)) {
    fail(`duplicate participantId ${participant.participantId}`);
  }
  participantsById.set(participant.participantId, participantIndex);
  const player = new context.Player(participantIndex, false, battle);
  const team = participant.team.map((pokemonSetup) => {
    const build = pokemonSetup.build;
    const pokemon = new context.Pokemon(build.speciesId, participantIndex, battle);
    pokemon.setIV("atk", build.ivs.attack);
    pokemon.setIV("def", build.ivs.defense);
    pokemon.setIV("hp", build.ivs.hp);
    pokemon.setLevel(build.level);
    if (build.shadow) {
      pokemon.setShadowType("shadow");
    }
    pokemon.selectMove("fast", pokemonSetup.fastMove);
    for (const [moveIndex, moveId] of pokemonSetup.chargedMoves.entries()) {
      pokemon.selectMove("charged", moveId, moveIndex);
    }
    if (pokemonSetup.chargedMoves.length === 0) {
      pokemon.selectMove("charged", "STRUGGLE", 0, true);
    }
    pokemon.setShields(participant.shieldsRemaining);
    if (pokemonSetup.startingHp !== undefined) {
      pokemon.setStartHp(pokemonSetup.startingHp);
    }
    if (pokemonSetup.startingEnergy !== undefined) {
      pokemon.setStartEnergy(pokemonSetup.startingEnergy);
    }
    return pokemon;
  });
  player.setTeam(team);
  players.push(player);
  activePokemon.push(team[0]);
}

battle.setPlayers(players);
for (const [participantIndex, pokemon] of activePokemon.entries()) {
  battle.setNewPokemon(pokemon, participantIndex);
}
battle.setSandboxMode(true);

const timelineActions = [];
for (const step of fixture.input.steps) {
  if (!Array.isArray(step.actionRequests)) {
    fail("each input.steps item must contain actionRequests");
  }
  const requestsByParticipant = new Map();
  for (const request of step.actionRequests) {
    if (!participantsById.has(request.participantId)) {
      fail(`unknown participantId ${request.participantId}`);
    }
    if (requestsByParticipant.has(request.participantId)) {
      fail(`duplicate action request for ${request.participantId} in one step`);
    }
    requestsByParticipant.set(request.participantId, request);
  }

  const requestTurns = new Set(step.actionRequests.map((request) => request.turn));
  if (requestTurns.size !== 1) {
    fail("all action requests in a step must have the same turn");
  }
  const requestTurn = step.actionRequests[0]?.turn;
  if (requestTurn === undefined) {
    fail("each step must have action requests for both participants");
  }

  const shieldsByParticipant = new Map(
    (step.shieldDecisions ?? []).map((decision) => [decision.participantId, decision]),
  );

  for (const [participantId, participantIndex] of participantsById) {
    const request = requestsByParticipant.get(participantId);
    if (!request) {
      fail(`missing action request for ${participantId} on turn ${requestTurn}`);
    }
    const actionType = request.actionType;
    const payload = request.payload ?? {};
    let timelineType;
    let value = -1;
    let forceBuff = false;
    let targetParticipantIndex = 1 - participantIndex;

    switch (actionType) {
      case "fastMove":
        timelineType = "fast";
        if (payload.moveId !== activePokemon[participantIndex].fastMove.moveId) {
          fail(`fast move ${payload.moveId} is not the selected PvPoke fast move`);
        }
        break;
      case "chargedMove": {
        timelineType = "charged";
        value = activePokemon[participantIndex].chargedMoves.findIndex(
          (move) => move.moveId === payload.moveId,
        );
        if (value < 0) {
          fail(`charged move ${payload.moveId} is not in the selected PvPoke moveset`);
        }
        const move = activePokemon[participantIndex].chargedMoves[value];
        if (move.buffs && Number(move.buffApplyChance) < 1) {
          fail(`chance-based move ${payload.moveId} cannot be matched across RNG implementations yet`);
        }
        forceBuff = Boolean(move.buffs && Number(move.buffApplyChance) === 1);
        break;
      }
      case "swap": {
        timelineType = "switch";
        const targetIndex = setup.participants[participantIndex].team.findIndex(
          (pokemon) => pokemon.slot === payload.targetSlot,
        );
        if (targetIndex < 0) {
          fail(`unknown swap target slot ${payload.targetSlot}`);
        }
        value = targetIndex;
        break;
      }
      case "wait":
        timelineType = "wait";
        break;
      default:
        fail(`unsupported actionType ${actionType}`);
    }

    if (actionType === "chargedMove") {
      const targetId = setup.participants[targetParticipantIndex].participantId;
      const shieldDecision = shieldsByParticipant.get(targetId);
      if (shieldDecision && shieldDecision.turn !== requestTurn) {
        fail(`shield decision for ${targetId} has the wrong turn`);
      }
      var shielded = shieldDecision?.decision === "shield";
    } else {
      var shielded = false;
    }

    timelineActions.push(new context.TimelineAction(
      timelineType,
      participantIndex,
      requestTurn + 1,
      value,
      {
        shielded,
        buffs: forceBuff,
        charge: 1,
        priority: activePokemon[participantIndex].priority,
      },
    ));
  }
}

battle.setActions(timelineActions);
const rawTimeline = battle.simulate();
const actionEventTypes = new Set(["tap interaction wait"]);
const turns = rawTimeline.reduce((maximum, event) => {
  if (
    event.type.startsWith("fast ") ||
    event.type.startsWith("charged ") ||
    actionEventTypes.has(event.type)
  ) {
    return Math.max(maximum, Number(event.turn) || 0);
  }
  return maximum;
}, 0);
const timeline = Array.from({ length: turns }, (_, turn) => ({
  turn,
  timestamp: turn * TURN_DURATION_MS,
  events: [],
}));
const shieldEvents = new Set();

for (const event of rawTimeline) {
  if (event.type === "shield") {
    shieldEvents.add(`${event.turn}:${event.actor}`);
  }
}

const lastDamageTurnByParticipant = new Map();

function moveForEvent(actor, event) {
  const pokemon = activePokemon[actor];
  const candidates = [pokemon.fastMove, ...pokemon.chargedMoves];
  const move = candidates.find((candidate) => candidate?.name === event.name);
  if (!move) {
    fail(`cannot map PvPoke event move ${event.name} to a selected move`);
  }
  return move;
}

for (const event of rawTimeline) {
  const actor = Number(event.actor);
  const turn = Number(event.turn) - 1;
  if (event.type === "shield") {
    continue;
  }
  if (event.type === "tap interaction") {
    continue;
  }
  if (event.type.startsWith("tap ") && event.type !== "tap interaction wait") continue;
  if (!Number.isInteger(actor) || actor < 0 || actor > 1) {
    fail(`invalid actor on PvPoke event ${event.type}`);
  }
  const resolvedTurn = event.type === "faint"
    ? lastDamageTurnByParticipant.get(actor) ?? Math.max(0, turn - 1)
    : event.type === "switchAvailable"
      ? Math.max(0, turn - 1)
    : turn;
  if (!Number.isInteger(resolvedTurn) || resolvedTurn < 0 || resolvedTurn >= timeline.length) {
    fail(`event turn ${event.turn} is outside the simulated battle`);
  }

  const participant = setup.participants[actor];
  let normalized;
  if (event.type.startsWith("fast ")) {
    const move = moveForEvent(actor, event);
    lastDamageTurnByParticipant.set(1 - actor, resolvedTurn);
    normalized = {
      kind: "tap_interaction",
      participantId: participant.participantId,
      targetParticipantId: setup.participants[1 - actor].participantId,
      moveId: move.moveId,
      damage: event.values?.[0],
      durationMs: move.cooldown,
    };
  } else if (event.type === "tap interaction wait") {
    normalized = {
      kind: "tap_interaction_wait",
      participantId: participant.participantId,
      targetParticipantId: setup.participants[1 - actor].participantId,
      moveId: activePokemon[actor].fastMove.moveId,
      durationMs: TURN_DURATION_MS,
    };
  } else if (event.type.startsWith("charged ")) {
    const move = moveForEvent(actor, event);
    const hasBuffEffect = Boolean(move.buffs);
    lastDamageTurnByParticipant.set(1 - actor, resolvedTurn);
    normalized = {
      kind: "charged",
      participantId: participant.participantId,
      targetParticipantId: setup.participants[1 - actor].participantId,
      moveId: move.moveId,
      damage: event.values?.[0],
      shielded: shieldEvents.has(`${event.turn}:${1 - actor}`),
      durationMs: move.cooldown,
    };
    if (hasBuffEffect) {
      normalized.buffApplied = event.type.includes(" buff") || event.type.includes(" debuff");
    }
  } else if (event.type === "switchAvailable") {
    normalized = {
      kind: "switch_available",
      participantId: participant.participantId,
    };
  } else if (event.type === "faint") {
    normalized = {
      kind: "faint",
      participantId: participant.participantId,
      species: activePokemon[actor].speciesId,
    };
  } else {
    fail(`unsupported PvPoke timeline event type ${event.type}`);
  }

  timeline[resolvedTurn].events.push(normalized);
}

const pvpokeRatings = battle.getBattleRatings();
const finalPokemon = battle.getPokemon();
const teamTotals = finalPokemon.map((pokemon) => ({
  maxHp: pokemon.stats.hp,
  finalHp: pokemon.hp,
}));
const participants = setup.participants.map((participant, index) => {
  const opponent = teamTotals[1 - index];
  const opponentDamage = opponent.maxHp - opponent.finalHp;
  return {
    participantId: participant.participantId,
    side: index === 0 ? "left" : "right",
    rating: pvpokeRatings[index],
    finalHp: teamTotals[index].finalHp,
    maxHp: teamTotals[index].maxHp,
    opponentDamage,
    opponentMaxHp: opponent.maxHp,
  };
});

let winnerIndex = null;
if (teamTotals[0].finalHp > 0 && teamTotals[1].finalHp === 0) {
  winnerIndex = 0;
} else if (teamTotals[1].finalHp > 0 && teamTotals[0].finalHp === 0) {
  winnerIndex = 1;
} else if (teamTotals[0].finalHp > 0 && teamTotals[1].finalHp > 0) {
  const winner = battle.getWinner()?.pokemon;
  if (winner) {
    winnerIndex = winner.index;
  }
}

fixture.expectedResult = {
  battleId: setup.battleId,
  outcome: winnerIndex === 0
    ? "participant_0_win"
    : winnerIndex === 1
      ? "participant_1_win"
      : "draw",
  winner: winnerIndex === null ? null : setup.participants[winnerIndex].participantId,
  loser: winnerIndex === null ? null : setup.participants[1 - winnerIndex].participantId,
  participants,
  timeline,
  totalTurns: turns,
  battleDuration: turns * TURN_DURATION_MS,
};

fixture.input.steps = fixture.input.steps.filter((step) =>
  step.actionRequests.some((request) => request.turn < turns),
);

const output = `${JSON.stringify(fixture, null, 2)}\n`;
const temporaryPath = `${outputPath}.tmp`;
writeFileSync(temporaryPath, output);
renameSync(temporaryPath, outputPath);
console.log(`Generated ${fixture.caseId}: ${turns} turns, ${rawTimeline.length} PvPoke events -> ${outputPath}`);