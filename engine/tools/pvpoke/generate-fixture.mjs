import { createHash } from "node:crypto";
import { readFileSync, renameSync, writeFileSync } from "node:fs";
import path from "node:path";
import vm from "node:vm";

const FIXTURE_VERSION = 1;
const GENERATOR_REVISION = "pvpoke-node-harness-v5";
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
if (!Array.isArray(fixture.input.steps) || fixture.input.steps.length === 0) {
  fail("input.steps must contain explicit action requests");
}

const participantIndexById = new Map();
const pokemonIdOwners = new Map();
for (const [participantIndex, participant] of setup.participants.entries()) {
  if (!participant.participantId || typeof participant.participantId !== "string") {
    fail(`participant ${participantIndex} is missing participantId`);
  }
  if (participantIndexById.has(participant.participantId)) {
    fail(`duplicate participantId ${participant.participantId}`);
  }
  participantIndexById.set(participant.participantId, participantIndex);
  if (!Array.isArray(participant.team) || participant.team.length === 0) {
    fail(`${participant.participantId} must have a non-empty team`);
  }
  for (const pokemonSetup of participant.team) {
    if (!pokemonSetup.pokemonId) {
      fail(`${participant.participantId} has a team member without pokemonId`);
    }
    if (pokemonIdOwners.has(pokemonSetup.pokemonId)) {
      fail(
        `duplicate pokemonId ${pokemonSetup.pokemonId} on ${participant.participantId} and ${pokemonIdOwners.get(pokemonSetup.pokemonId)}`,
      );
    }
    pokemonIdOwners.set(pokemonSetup.pokemonId, participant.participantId);
    if (!Array.isArray(pokemonSetup.chargedMoves)) {
      fail(`${pokemonSetup.pokemonId} must have a chargedMoves array`);
    }
  }
}

const battle = new context.Battle();
battle.setCP(setup.cpLimit ?? 1500);
battle.setLevelCap(setup.levelCap ?? 50);

function applyStartingOverrides(pokemon, pokemonSetup) {
  if (pokemonSetup.startingHp !== undefined) {
    pokemon.setStartHp(pokemonSetup.startingHp);
  }
  if (pokemonSetup.startingEnergy !== undefined) {
    pokemon.setStartEnergy(pokemonSetup.startingEnergy);
  }
}

// PvPoke's sandbox action lookup only considers a switch whose team index is
// smaller than the outgoing Pokemon's charged-move array. Adapter-only padding
// keeps arbitrary team slots addressable; padded moves are never scripted.
function padChargedMovesForSwitching(pokemon, teamSize) {
  if (pokemon.chargedMoves.length >= teamSize) {
    return;
  }
  const dummy = gameMaster.getMoveById("STRUGGLE");
  if (!dummy) {
    fail("STRUGGLE is required to pad charged moves for team switching");
  }
  while (pokemon.chargedMoves.length < teamSize) {
    pokemon.chargedMoves.push(structuredClone(dummy));
  }
}

function createPokemon(participantIndex, pokemonSetup) {
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
  return pokemon;
}

const players = [];
const teams = [];
for (const [participantIndex, participant] of setup.participants.entries()) {
  const player = new context.Player(participantIndex, false, battle);
  const team = participant.team.map((pokemonSetup) =>
    createPokemon(participantIndex, pokemonSetup),
  );
  for (const [teamIndex, pokemon] of team.entries()) {
    pokemon.setShields(participant.shieldsRemaining);
    applyStartingOverrides(pokemon, participant.team[teamIndex]);
    padChargedMovesForSwitching(pokemon, team.length);
  }
  player.setTeam(team);
  players.push(player);
  teams.push(team);
}

battle.setPlayers(players);
for (const [participantIndex, participant] of setup.participants.entries()) {
  battle.setNewPokemon(teams[participantIndex][0], participantIndex);

  // Set these after initialization: setNewPokemon resets both start values.
  applyStartingOverrides(teams[participantIndex][0], participant.team[0]);
  padChargedMovesForSwitching(teams[participantIndex][0], participant.team.length);
}
battle.setSandboxMode(true);
battle.start();

// ---------------------------------------------------------------------------
// Step expansion and validation
// ---------------------------------------------------------------------------

const steps = [];
let expectedTurn = 0;
const shieldDecisionsByParticipantTurn = new Map();

for (const template of fixture.input.steps) {
  if (!Array.isArray(template.actionRequests) || template.actionRequests.length === 0) {
    fail("each input.steps item must contain actionRequests");
  }
  const stepKind = template.stepKind ?? "turn";
  if (stepKind !== "turn" && stepKind !== "forcedSwitch") {
    fail(`unsupported stepKind ${stepKind}`);
  }
  const isContinuation = stepKind === "forcedSwitch";
  const repeatCount = template.repeatCount ?? 1;

  if (isContinuation) {
    if (template.repeatCount !== undefined) {
      fail("forcedSwitch steps cannot use repeatCount");
    }
    if ((template.shieldDecisions ?? []).length > 0) {
      fail("forcedSwitch steps cannot contain shield decisions");
    }
  } else if (!Number.isInteger(repeatCount) || repeatCount < 1) {
    fail("step repeatCount must be a positive integer");
  }

  const baseTurns = new Set(template.actionRequests.map((request) => request.turn));
  if (baseTurns.size !== 1) {
    fail("all action requests in a step must have the same turn");
  }
  const baseTurn = template.actionRequests[0].turn;
  if (!Number.isInteger(baseTurn) || baseTurn < 0) {
    fail(`step turn ${baseTurn} must be a non-negative integer`);
  }
  if (baseTurn !== expectedTurn) {
    fail(
      `step turn ${baseTurn} does not match the next resolver turn ${expectedTurn}; normal steps must cover consecutive turns`,
    );
  }

  const requestsByParticipant = new Map();
  for (const request of template.actionRequests) {
    if (!participantIndexById.has(request.participantId)) {
      fail(`unknown participantId ${request.participantId}`);
    }
    if (requestsByParticipant.has(request.participantId)) {
      fail(`duplicate action request for ${request.participantId} in one step`);
    }
    requestsByParticipant.set(request.participantId, request);
  }

  if (isContinuation) {
    for (const request of template.actionRequests) {
      if (request.actionType !== "swap") {
        fail("forcedSwitch steps may only contain swap action requests");
      }
      const targetSlot = request.payload?.targetSlot;
      if (!Number.isInteger(targetSlot)) {
        fail("forcedSwitch steps must select an integer targetSlot");
      }
      const participantIndex = participantIndexById.get(request.participantId);
      if (targetSlot < 0 || targetSlot >= setup.participants[participantIndex].team.length) {
        fail(
          `targetSlot ${targetSlot} is outside ${request.participantId}'s team roster`,
        );
      }
    }
    steps.push({
      kind: stepKind,
      turn: baseTurn,
      actionRequests: template.actionRequests.map((request) => ({ ...request })),
      shieldDecisions: [],
    });
    continue;
  }

  for (const [participantId] of participantIndexById) {
    if (!requestsByParticipant.has(participantId)) {
      fail(`missing action request for ${participantId} on turn ${baseTurn}`);
    }
  }

  const swapParticipants = template.actionRequests.filter(
    (request) => request.actionType === "swap",
  );
  if (
    swapParticipants.length > 0 &&
    template.actionRequests.some(
      (request) => request.actionType === "fastMove" || request.actionType === "chargedMove",
    )
  ) {
    fail(
      "a fixture cannot combine a voluntary swap with an incoming damaging move in the same step: PvPoke's simulation targets pre-switch references, while the engine resolves swaps before damage. Swap both sides or wait with the opponent.",
    );
  }

  for (let offset = 0; offset < repeatCount; offset += 1) {
    const turn = baseTurn + offset;
    const actionRequests = template.actionRequests.map((request) => ({
      ...request,
      turn,
    }));
    const shieldDecisions = (template.shieldDecisions ?? []).map((decision) => ({
      ...decision,
      turn: decision.turn + offset,
    }));
    for (const decision of shieldDecisions) {
      if (!participantIndexById.has(decision.participantId)) {
        fail(`unknown shield decision participantId ${decision.participantId}`);
      }
      if (!Number.isInteger(decision.turn) || decision.turn < 0) {
        fail(`shield decision turn ${decision.turn} must be a non-negative integer`);
      }
      if (decision.decision !== "shield" && decision.decision !== "no_shield") {
        fail('shield decision must be "shield" or "no_shield"');
      }
      const key = `${decision.participantId}:${decision.turn}`;
      if (shieldDecisionsByParticipantTurn.has(key)) {
        fail(
          `duplicate shield decision for ${decision.participantId} on turn ${decision.turn}`,
        );
      }
      shieldDecisionsByParticipantTurn.set(key, decision);
    }
    steps.push({
      kind: stepKind,
      turn,
      actionRequests,
      shieldDecisions,
    });
    expectedTurn += 1;
  }
}

// ---------------------------------------------------------------------------
// Stepped simulation
// ---------------------------------------------------------------------------

function participantIndex(participantId) {
  return participantIndexById.get(participantId);
}

function participantId(participantIndexValue) {
  return setup.participants[participantIndexValue].participantId;
}

function livingBench(participantIndexValue, activePokemon) {
  return teams[participantIndexValue]
    .map((pokemon, teamIndex) => ({ pokemon, teamIndex }))
    .filter(({ pokemon }) => pokemon !== activePokemon && pokemon.hp > 0);
}

function syncShieldsFromActive(participantIndexValue, activePokemon) {
  const shields = activePokemon.shields;
  for (const pokemon of teams[participantIndexValue]) {
    pokemon.setShields(shields);
  }
}

const actions = [];
const activeAtTurnStart = [];
const voluntarySwapsByTurn = new Map();
const replacementQueues = new Map();
let turnCount = 0;
let finished = false;
const expectedShieldDecisionKeys = new Set();

function recordReplacement(participantIndexValue, speciesId) {
  const id = participantId(participantIndexValue);
  if (!replacementQueues.has(id)) {
    replacementQueues.set(id, []);
  }
  replacementQueues.get(id).push(speciesId);
}

function installReplacement(participantIndexValue, teamIndex) {
  const incoming = teams[participantIndexValue][teamIndex];
  battle.setNewPokemon(incoming, participantIndexValue, false);
  padChargedMovesForSwitching(incoming, teams[participantIndexValue].length);
  recordReplacement(participantIndexValue, incoming.speciesId);
}

let stepIndex = 0;
while (stepIndex < steps.length) {
  const step = steps[stepIndex];
  if (step.kind === "forcedSwitch") {
    fail(
      `forcedSwitch step for turn ${step.turn} was not preceded by a faint that left multiple living replacements`,
    );
  }

  if (battle.getTurns() !== step.turn + 1) {
    fail(
      `reference battle is on turn ${battle.getTurns()}, expected turn ${step.turn + 1} for fixture turn ${step.turn}`,
    );
  }

  const activeBefore = battle.getPokemon().slice();
  const requestsByParticipant = new Map(
    step.actionRequests.map((request) => [request.participantId, request]),
  );
  const swapTargets = new Map();

  for (const [id, request] of requestsByParticipant) {
    const participantIndexValue = participantIndex(id);
    const activePokemon = activeBefore[participantIndexValue];
    const activeTeamIndex = teams[participantIndexValue].indexOf(activePokemon);
    if (activeTeamIndex < 0) {
      fail(`active Pokémon for ${id} is not on its team roster`);
    }
    const activeSetup = setup.participants[participantIndexValue].team[activeTeamIndex];
    const actionType = request.actionType;
    const payload = request.payload ?? {};
    let timelineType;
    let value = -1;
    let forceBuff = false;
    let targetParticipantIndex = 1 - participantIndexValue;

    switch (actionType) {
      case "fastMove":
        timelineType = "fast";
        if (payload.moveId !== activeSetup.fastMove) {
          fail(`fast move ${payload.moveId} is not the selected fixture fast move`);
        }
        if (payload.moveId !== activePokemon.fastMove.moveId) {
          fail(`fast move ${payload.moveId} is not the selected PvPoke fast move`);
        }
        break;
      case "chargedMove": {
        timelineType = "charged";
        if (!activeSetup.chargedMoves.includes(payload.moveId)) {
          fail(`charged move ${payload.moveId} is not in the fixture moveset`);
        }
        value = activePokemon.chargedMoves.findIndex(
          (move) => move.moveId === payload.moveId,
        );
        if (value < 0) {
          fail(`charged move ${payload.moveId} is not in the selected PvPoke moveset`);
        }
        const move = activePokemon.chargedMoves[value];
        if (move.buffs && Number(move.buffApplyChance) < 1) {
          fail(
            `chance-based move ${payload.moveId} cannot be matched across RNG implementations yet`,
          );
        }
        forceBuff = Boolean(move.buffs && Number(move.buffApplyChance) === 1);
        break;
      }
      case "swap": {
        timelineType = "switch";
        const targetSlot = payload.targetSlot;
        if (!Number.isInteger(targetSlot)) {
          fail("swap targetSlot must be an integer");
        }
        if (targetSlot < 0 || targetSlot >= teams[participantIndexValue].length) {
          fail(`unknown swap target slot ${targetSlot}`);
        }
        const target = teams[participantIndexValue][targetSlot];
        if (target === activePokemon) {
          fail(`swap target slot ${targetSlot} is already active`);
        }
        if (target.hp <= 0) {
          fail(`swap target slot ${targetSlot} has fainted`);
        }
        value = targetSlot;
        swapTargets.set(participantIndexValue, targetSlot);
        break;
      }
      case "wait":
        timelineType = "wait";
        break;
      default:
        fail(`unsupported actionType ${actionType}`);
    }

    let shielded = false;
    if (actionType === "chargedMove") {
      const targetId = participantId(targetParticipantIndex);
      const shieldDecisionKey = `${targetId}:${step.turn + 1}`;
      expectedShieldDecisionKeys.add(shieldDecisionKey);
      const shieldDecision = shieldDecisionsByParticipantTurn.get(shieldDecisionKey);
      shielded = shieldDecision?.decision === "shield";
    }

    actions.push(
      new context.TimelineAction(
        timelineType,
        participantIndexValue,
        step.turn + 1,
        value,
        {
          shielded,
          buffs: forceBuff,
          charge: 1,
          priority: activePokemon.priority,
        },
      ),
    );
  }

  battle.setActions(actions);
  activeAtTurnStart.push(activeBefore);
  battle.step();
  turnCount += 1;
  const activeAfter = battle.getPokemon().slice();

  for (const [participantIndexValue, targetSlot] of swapTargets) {
    if (activeAfter[participantIndexValue] !== teams[participantIndexValue][targetSlot]) {
      fail(
        `PvPoke rejected ${participantId(participantIndexValue)}'s swap to slot ${targetSlot}; check the switch cooldown or team roster`,
      );
    }
    if (!voluntarySwapsByTurn.has(step.turn)) {
      voluntarySwapsByTurn.set(step.turn, []);
    }
    voluntarySwapsByTurn.get(step.turn).push({
      participantIndex: participantIndexValue,
      species: activeAfter[participantIndexValue].speciesId,
    });
  }

  for (const [participantIndexValue, activePokemon] of activeAfter.entries()) {
    syncShieldsFromActive(participantIndexValue, activePokemon);
  }

  const pendingChoices = new Map();
  for (const [participantIndexValue, activePokemon] of activeAfter.entries()) {
    if (activePokemon.hp > 0) {
      continue;
    }
    const bench = livingBench(participantIndexValue, activePokemon);
    if (bench.length === 0) {
      continue;
    }
    pendingChoices.set(
      participantIndexValue,
      bench.length === 1 ? bench[0].teamIndex : null,
    );
  }

  while (
    pendingChoices.size > 0 &&
    stepIndex + 1 < steps.length &&
    steps[stepIndex + 1].kind === "forcedSwitch"
  ) {
    const continuation = steps[stepIndex + 1];
    if (continuation.turn !== turnCount) {
      fail(
        `forcedSwitch step turn ${continuation.turn} must match the next resolver turn ${turnCount}`,
      );
    }
    for (const request of continuation.actionRequests) {
      const participantIndexValue = participantIndex(request.participantId);
      if (!pendingChoices.has(participantIndexValue)) {
        fail(
          `forcedSwitch step selects a replacement for ${request.participantId}, but that participant has no pending choice`,
        );
      }
      if (pendingChoices.get(participantIndexValue) !== null) {
        fail(
          `duplicate forcedSwitch choice for ${request.participantId} on turn ${turnCount}`,
        );
      }
      const targetSlot = request.payload.targetSlot;
      const target = teams[participantIndexValue][targetSlot];
      if (!target || target.hp <= 0) {
        fail(
          `forcedSwitch target slot ${targetSlot} for ${request.participantId} is not a living bench Pokémon`,
        );
      }
      if (target === activeAfter[participantIndexValue]) {
        fail(
          `forcedSwitch target slot ${targetSlot} for ${request.participantId} is the fainted active Pokémon`,
        );
      }
      pendingChoices.set(participantIndexValue, targetSlot);
    }
    stepIndex += 1;
  }

  for (const [participantIndexValue, choice] of pendingChoices) {
    if (choice === null) {
      fail(
        `${participantId(participantIndexValue)} has multiple living replacements but no forcedSwitch step supplies a choice`,
      );
    }
    installReplacement(participantIndexValue, choice);
  }

  if (teams.some((team) => !team.some((pokemon) => pokemon.hp > 0))) {
    // PvPoke applies charged-move damage in the step where the move is queued,
    // while the engine resolves queued charged moves at the start of the next
    // turn. When a charged move wipes a team, consume the following normal step
    // as the turn that resolves it so the normalized timeline and total turn
    // count match the engine. The reference has already stopped, so that step
    // contributes no new PvPoke events.
    const chargedFaint = activeAfter.some(
      (pokemon) => pokemon.hp <= 0 && pokemon.faintSource === "charged",
    );
    if (chargedFaint) {
      const nextStep = steps[stepIndex + 1];
      if (
        !nextStep ||
        nextStep.kind !== "turn" ||
        nextStep.turn !== turnCount
      ) {
        fail(
          `a charged move finished the battle; add a normal step for turn ${turnCount} so the engine can resolve it`,
        );
      }
      activeAtTurnStart.push(activeAfter);
      turnCount += 1;
      stepIndex += 1;
    }
    finished = true;
    break;
  }

  stepIndex += 1;
}

for (const [key, decision] of shieldDecisionsByParticipantTurn) {
  if (!expectedShieldDecisionKeys.has(key)) {
    fail(
      `shield decision for ${decision.participantId} on turn ${decision.turn} has no incoming charged move`,
    );
  }
}

if (!finished) {
  fail("action stream did not finish the battle; add steps or increase repeatCount");
}

// ---------------------------------------------------------------------------
// Timeline normalization
// ---------------------------------------------------------------------------

const timeline = Array.from({ length: turnCount }, (_, turn) => ({
  turn,
  timestamp: turn * TURN_DURATION_MS,
  events: [],
}));

for (const [turn, swaps] of voluntarySwapsByTurn) {
  swaps.sort((a, b) => a.participantIndex - b.participantIndex);
  for (const swap of swaps) {
    timeline[turn].events.push({
      kind: "swap",
      participantId: participantId(swap.participantIndex),
      species: swap.species,
    });
  }
}

const chargedResolutionTurns = new Set(
  steps
    .filter((step) => step.kind === "turn")
    .flatMap((step) =>
      step.actionRequests
        .filter((request) => request.actionType === "chargedMove")
        .map((request) => `${request.participantId}:${request.turn + 1}`),
    ),
);
const shieldEvents = new Set();
const rawTimeline = battle.getTimeline();

for (const event of rawTimeline) {
  if (event.type === "shield") {
    shieldEvents.add(`${event.turn}:${event.actor}`);
  }
}

const lastDamageTurnByParticipant = new Map();

function moveForEvent(actor, turn, event) {
  const pokemon = activeAtTurnStart[turn]?.[actor];
  if (!pokemon) {
    fail(`cannot identify the active Pokémon for ${event.type} on turn ${turn}`);
  }
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
  if (event.type === "shield" || event.type === "tap interaction") {
    continue;
  }
  if (event.type.startsWith("tap ") && event.type !== "tap interaction wait") {
    continue;
  }
  if (!Number.isInteger(actor) || actor < 0 || actor > 1) {
    fail(`invalid actor on PvPoke event ${event.type}`);
  }
  let resolvedTurn = turn;
  if (event.type.startsWith("charged ")) {
    resolvedTurn += 1;
  } else if (event.type === "faint") {
    resolvedTurn = lastDamageTurnByParticipant.get(actor) ?? Math.max(0, turn - 1);
  } else if (event.type === "switchAvailable") {
    resolvedTurn = Math.max(0, turn - 1);
  }
  if (!Number.isInteger(resolvedTurn) || resolvedTurn < 0 || resolvedTurn >= timeline.length) {
    fail(
      `PvPoke event ${event.type} resolves on turn ${resolvedTurn}, outside the ${timeline.length} processed turn(s)`,
    );
  }

  const participant = setup.participants[actor];
  if (
    event.type === "tap interaction wait" &&
    chargedResolutionTurns.has(`${participant.participantId}:${resolvedTurn}`)
  ) {
    continue;
  }

  let normalized;
  if (event.type.startsWith("fast ")) {
    const move = moveForEvent(actor, resolvedTurn, event);
    lastDamageTurnByParticipant.set(1 - actor, resolvedTurn);
    normalized = {
      kind: "tap_interaction",
      participantId: participant.participantId,
      targetParticipantId: participantId(1 - actor),
      moveId: move.moveId,
      damage: event.values?.[0],
      durationMs: move.cooldown,
    };
  } else if (event.type === "tap interaction wait") {
    const pokemon = activeAtTurnStart[resolvedTurn]?.[actor];
    if (!pokemon) {
      fail(`cannot identify the active Pokémon for a wait event on turn ${resolvedTurn}`);
    }
    normalized = {
      kind: "tap_interaction_wait",
      participantId: participant.participantId,
      targetParticipantId: participantId(1 - actor),
      moveId: pokemon.fastMove.moveId,
      durationMs: TURN_DURATION_MS,
    };
  } else if (event.type.startsWith("charged ")) {
    const initiatedTurn = Math.max(0, resolvedTurn - 1);
    const move = moveForEvent(actor, initiatedTurn, event);
    const hasBuffEffect = Boolean(move.buffs);
    lastDamageTurnByParticipant.set(1 - actor, resolvedTurn);
    normalized = {
      kind: "charged",
      participantId: participant.participantId,
      targetParticipantId: participantId(1 - actor),
      moveId: move.moveId,
      damage: event.values?.[0],
      shielded: shieldEvents.has(`${event.turn}:${1 - actor}`),
      durationMs: move.cooldown,
    };
    if (hasBuffEffect) {
      normalized.buffApplied =
        event.type.includes(" buff") || event.type.includes(" debuff");
    }
  } else if (event.type === "switchAvailable") {
    normalized = {
      kind: "switch_available",
      participantId: participant.participantId,
    };
  } else if (event.type === "faint") {
    const pokemon = activeAtTurnStart[resolvedTurn]?.[actor];
    normalized = {
      kind: "faint",
      participantId: participant.participantId,
      species: pokemon?.speciesId,
    };
  } else {
    fail(`unsupported PvPoke timeline event type ${event.type}`);
  }

  timeline[resolvedTurn].events.push(normalized);

  if (event.type === "faint") {
    const species = replacementQueues.get(participant.participantId)?.shift();
    if (species !== undefined) {
      timeline[resolvedTurn].events.push({
        kind: "swap",
        participantId: participant.participantId,
        species,
      });
    }
  }
}

// ---------------------------------------------------------------------------
// Result construction
// ---------------------------------------------------------------------------

const teamTotals = teams.map((team) => ({
  maxHp: team.reduce((sum, pokemon) => sum + pokemon.stats.hp, 0),
  finalHp: team.reduce((sum, pokemon) => sum + Math.max(0, pokemon.hp), 0),
}));

if (teamTotals[0].finalHp > 0 && teamTotals[1].finalHp === 0) {
  // participant 0 wins
} else if (teamTotals[1].finalHp > 0 && teamTotals[0].finalHp === 0) {
  // participant 1 wins
} else if (teamTotals[0].finalHp > 0 && teamTotals[1].finalHp > 0) {
  fail("action stream did not finish the battle; both teams still have living Pokémon");
} else if (teamTotals[0].finalHp > 0 || teamTotals[1].finalHp > 0) {
  fail("inconsistent final team HP totals");
}

function ratingFor(participantIndexValue) {
  const { maxHp, finalHp } = teamTotals[participantIndexValue];
  const opponent = teamTotals[1 - participantIndexValue];
  const opponentDamage = opponent.maxHp - opponent.finalHp;
  return Math.floor(
    500 * (finalHp / maxHp + opponentDamage / opponent.maxHp),
  );
}

const participants = setup.participants.map((participant, index) => {
  const { maxHp, finalHp } = teamTotals[index];
  const opponent = teamTotals[1 - index];
  return {
    participantId: participant.participantId,
    side: index === 0 ? "left" : "right",
    rating: ratingFor(index),
    finalHp,
    maxHp,
    opponentDamage: opponent.maxHp - opponent.finalHp,
    opponentMaxHp: opponent.maxHp,
  };
});

let winnerIndex = null;
if (teamTotals[0].finalHp > 0 && teamTotals[1].finalHp === 0) {
  winnerIndex = 0;
} else if (teamTotals[1].finalHp > 0 && teamTotals[0].finalHp === 0) {
  winnerIndex = 1;
}

fixture.expectedResult = {
  battleId: setup.battleId,
  outcome:
    winnerIndex === 0
      ? "participant_0_win"
      : winnerIndex === 1
        ? "participant_1_win"
        : "draw",
  winner: winnerIndex === null ? null : setup.participants[winnerIndex].participantId,
  loser: winnerIndex === null ? null : setup.participants[1 - winnerIndex].participantId,
  participants,
  timeline,
  totalTurns: turnCount,
  battleDuration: turnCount * TURN_DURATION_MS,
};

const output = `${JSON.stringify(fixture, null, 2)}\n`;
const temporaryPath = `${outputPath}.tmp`;
writeFileSync(temporaryPath, output);
renameSync(temporaryPath, outputPath);
console.log(
  `Generated ${fixture.caseId}: ${turnCount} turns, ${rawTimeline.length} PvPoke events -> ${outputPath}`,
);
