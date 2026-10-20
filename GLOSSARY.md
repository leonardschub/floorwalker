# Floorwalker

Canonical language for Floorwalker's appearance, legal-document, world and agent surfaces. The app runs entirely on the user's machine: there is no account and no service.

## Legal documents

**Legal document**:
A complete prose document the app publishes. There is one, the open-source licences page, which bundled third-party code requires whenever the binary is shipped; it carries its own title, last-updated date and language. The licence the app itself is published under (AGPL-3.0-or-later) is shown from the repository's own `LICENSE`, and the credit and licence notices in Settings' About section are interface text, not legal documents.
_Avoid_: legal page, legal copy, legal strings

**Document language**:
The language a legal document's prose is written in, which is independent of the interface language a visitor is using.
_Avoid_: locale, translation locale

## Appearance

**Dark-first**:
The app's default appearance is dark, and the light appearance is a complete alternative rather than an inversion of it.
_Avoid_: dark mode, night theme

**Theme preference**:
The device-local choice between light, dark and following the operating system, held on the device by the webview (or the browser), never by a service.
_Avoid_: theme setting, theme mode

**Resolved mode**:
The appearance actually painted, light or dark: the theme preference when it names one, and the operating system's choice when it follows the system.
_Avoid_: active theme, current theme

## World

**Dashboard**:
Not a screen the app has: it opens straight into the world screen. Earlier versions opened on a dashboard with an **Enter world** button.
_Avoid_: dashboard, for any screen of the app

**World**:
The 3D environment the desktop app opens into.

**World screen**:
The desktop screen the app opens on: the world view framed by thin top and bottom bars and a side panel. Its **Settings** button leads to Settings, whose way back returns to it.

**World view**:
The region of the desktop app's main window where the world is shown, framed by the 2D interface around it. There is no separate world window.
_Avoid_: world window, canvas

**Keep warm**:
The world, once started, stays alive while the user is away from the world screen, such as in Settings, so returning to it is instant. Only quitting stops it.
_Avoid_: background mode, pause

**Captured / released**:
Whether the cursor is locked into the world view (captured, so pointer movement acts on the world) or free to move over the rest of the app (released).
_Avoid_: pause, grab, focus

**Suspended**:
The world is not drawn because nobody can see it, such as while the window is minimised, yet keeps running. Distinct from the world being stopped.
_Avoid_: paused, stopped

## Agents

**Model**:
The language model an agent runs, such as Opus 5.5.

**Harness**:
The command-line tool that runs a model as an agent, such as Claude Code.
_Avoid_: client, CLI

**Session**:
One run of a harness. Every session forms a tree with exactly one root agent.

**Agent**:
A participant in a session that runs a model inside a harness. In the world, each agent sits at its own desk.
_Avoid_: using "agent" for a model

**Root agent**:
The agent a session starts with, at the top of the session's tree.

**Subagent**:
An agent spawned by another agent in the same session, shown as that agent's child in the tree.

**Project**:
A working directory the user runs sessions in. In the world, each project will have its own room.

**Open project**:
A project with at least one live session. Only open projects have a room in the world.
_Avoid_: active project

**Room**:
The space in the world that belongs to one open project, shared by all of its sessions. Rooms appear and disappear as projects open and close, and grow or shrink with the agents inside. A room is bigger on the inside: its doorway sits a short step from its neighbours' in the corridor, yet the room behind it is as large as its agents need, and looking or stepping through the doorway is seamless.
_Avoid_: office (the whole world), level

**Corridor**:
The walkway in the world. Project rooms' doorways stand side by side along one side, in the order their projects opened; management rooms' doorways face them from the other side, centred on its middle. It grows and shrinks with the number of open projects.
_Avoid_: hallway, lobby

**Desk**:
An agent's place in its project's room: a desk, a chair and a PC, with the agent's robot seated at it. Only a running agent has one: a live session's root agent and its running subagents. A desk goes the moment its agent finishes or its session ends; the side panel still lists finished sessions.
_Avoid_: workstation, seat

**Robot**:
The figure that personifies an agent at its desk. Its face shows a pattern in the colour of the company behind the agent's model, with the model's name, and it is smaller the deeper the agent sits in its session's tree.
_Avoid_: avatar, stick figure, bot

**Clutter**:
Props in a room that exist only for atmosphere, such as trash, crates and haze, as opposed to rooms, desks and robots. The user can switch clutter off.
_Avoid_: decoration, props

**Management room**:
A room on the far side of the corridor from the project rooms that belongs to the user rather than to any project. It holds no desks; it is where the user oversees and punishes agents.
_Avoid_: control room, boss room

**CCTV room**:
The management room where the user watches every project room through camera feeds and sees which agents are flagged.
_Avoid_: security room, monitor room

**Camera feed**:
One project room's live view, shown on a monitor in the CCTV room.
_Avoid_: stream, screen

**Monitor wall**:
The grid of camera feeds in the CCTV room, one per open project room, in the order of the corridor; past twelve it widens, and the room with it.
_Avoid_: video wall, screens

**Focus monitor**:
A large monitor in the CCTV room showing one flagged agent close up, above the monitor wall.
_Avoid_: main screen, zoom

**Ticker**:
The strip under the monitor wall listing every flagged agent.
_Avoid_: marquee, feed (a camera feed is something else)

**Flag**:
A mark on a live root agent that needs the user: **waiting on you** (it is idle, or blocked on a dialog) or **asking you something** (it is waiting on the user's answer to a question, a permission prompt or another request). A subagent's request flags its root agent.
_Avoid_: alert, warning, slacking

**Punishment**:
A show the user stages against an agent's robot from the CCTV room, which plays in the agent's project room. It is either a kill or work harder. Each also reaches the real session: work harder sends it a prompt at the bolt; a kill ends it at the blast.
_Avoid_: terminate, nudge

**Kill**:
The punishment in which a squad of robots storms the agent's room and blows its robot up, leaving a wreck, and which ends the agent's real session at the blast (only root agents; the whole session). Once fired it always goes through, unless the session ends or the desk goes first. Its console button reads KILL.
_Avoid_: strike-down (ambiguous), explode

**Work harder**:
The punishment in which lightning strikes the agent's robot, which then gets straight back to work, and which sends "WORK HARDER" into the agent's real session as a prompt at the bolt (only root agents; once). Its console button reads WORK HARDER.
_Avoid_: strike-down (ambiguous), zap, lightning strike (that is how it looks, not what it is called)

**Wreck**:
What is left at a desk after a kill. It stays until the agent's state next changes, then the robot is back.
_Avoid_: corpse, debris

**Session terminal**:
A live session's terminal as the harness draws it, shown on the PC at its root agent's desk and typed into from the seated view. Subagents run inside their root agent's session terminal and have none of their own.
_Avoid_: agent's screen, CLI, console (the CCTV room's console is something else)

**Live screen** / **Frozen screen**:
How a PC shows its session terminal. Live: watched now, in full colour; only the seated session's and the three nearest the user in the room they stand in are (the **live budget**). Frozen: the last picture the app saw, greyed and dimmed. A PC whose session terminal the app has never seen stays plainly lit.
_Avoid_: active / inactive screen, paused (the session itself keeps running)

**Sit in**:
To take an attachable root agent's seat: the camera goes into its robot's eyes and the seated view begins. The user sits in by right-clicking the robot or its desk within reach, by right-clicking its row in the side panel (or pressing the context-menu key or Shift+F10 on the focused row), by pressing SIT IN on the CCTV console, and by starting a New session, which sits in the session it started. Sitting in another agent while seated goes straight from seat to seat. An agent that can't be sat in gets a hint instead: a terminal-started root agent, or a subagent.
_Avoid_: attach (what the app does underneath), possess, take over, enter

**Seated view**:
Looking through a robot's eyes at its PC, with the user's keys going to its session terminal: what sitting in begins. The user leaves it by right-clicking the world view or pressing Leave, and it ends by itself when the session's attach leaves the session or the session ends; leaving puts the user back where they stood.
_Avoid_: POV, becoming the agent, possess

**Attachable**:
Whether the app can show and type into a session's terminal: true of a live background session, never of a session running in a terminal the user opened. Only an attachable session's root agent has a session terminal on its PC.
_Avoid_: connectable, controllable

**Dormant**:
A background session whose process has been shut down while it sat idle and unwatched. Its conversation is kept, but it no longer counts as live, so like any finished agent it leaves the world and the side panel at once.
_Avoid_: asleep, stopped, retired

**Live** / **Finished**:
Whether a session is still running. A live root agent is **busy** (working), **waiting** (blocked until the user answers something) or **idle** (finished its turn, waiting for the user's next instruction); a subagent is either running or finished.
_Avoid_: active, online, done

**Side panel**:
The flat, simplified view of the world beside the world view on the world screen, offering switchable views of the user's agents and their usage.

**Agent tree**:
The side-panel view listing every live session as a tree of its root agent and its running subagents.

**Model of the month** / **Harness of the month**:
The model and the harness that stand out in the user's own usage during the current month.
_Avoid_: agent of the month
