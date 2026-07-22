import { afterEach, describe, expect, it, vi } from 'vitest';

import { InputManager } from './input';
import { MediaTransport, type MediaDependencies, type MediaView } from './media';
import type { RealtimeView } from './realtime';
import { settingsSnapshot, stateSnapshot } from './test-fixtures';
import type { ClientMessage, ServerMessage } from './wire';

type MessageSubscriber = (message: ServerMessage) => void;
type FrameSubscriber = (frame: ArrayBuffer | Blob) => void;
type ViewSubscriber = (view: RealtimeView) => void;

function realtimeView(
  overrides: Partial<RealtimeView> = {},
  stunServer = 'stun:relay.example.test:3478'
): RealtimeView {
  return {
    attempts: 0,
    lastError: null,
    latestRevision: '0',
    maxRetries: 20,
    settings: settingsSnapshot('0', { stun_server: stunServer }),
    state: stateSnapshot('0'),
    status: 'connected',
    ...overrides
  };
}

class FakeRealtime {
  readonly sent: ClientMessage[] = [];
  private readonly frameSubscribers = new Set<FrameSubscriber>();
  private readonly messageSubscribers = new Set<MessageSubscriber>();
  private readonly viewSubscribers = new Set<ViewSubscriber>();

  constructor(private view: RealtimeView = realtimeView()) {}

  onFrame(subscriber: FrameSubscriber): () => void {
    this.frameSubscribers.add(subscriber);
    return () => {
      this.frameSubscribers.delete(subscriber);
    };
  }

  onMessage(subscriber: MessageSubscriber): () => void {
    this.messageSubscribers.add(subscriber);
    return () => {
      this.messageSubscribers.delete(subscriber);
    };
  }

  send(message: ClientMessage): boolean {
    this.sent.push(message);
    return true;
  }

  subscribe(subscriber: ViewSubscriber): () => void {
    this.viewSubscribers.add(subscriber);
    subscriber(this.view);
    return () => {
      this.viewSubscribers.delete(subscriber);
    };
  }

  emitFrame(frame: ArrayBuffer | Blob): void {
    for (const subscriber of this.frameSubscribers) {
      subscriber(frame);
    }
  }

  emitMessage(message: ServerMessage): void {
    for (const subscriber of this.messageSubscribers) {
      subscriber(message);
    }
  }

  emitView(view: RealtimeView): void {
    this.view = view;
    for (const subscriber of this.viewSubscribers) {
      subscriber(view);
    }
  }
}

class FakeDataChannel {
  binaryType: BinaryType = 'blob';
  bufferedAmount = 0;
  readonly closed: boolean[] = [];
  maxPacketLifeTime: number | null = null;
  maxRetransmits: number | null = null;
  onclose: RTCDataChannel['onclose'] = null;
  onerror: RTCDataChannel['onerror'] = null;
  onmessage: RTCDataChannel['onmessage'] = null;
  onopen: RTCDataChannel['onopen'] = null;
  ordered = true;
  readyState: RTCDataChannelState = 'connecting';
  readonly sent: string[] = [];

  constructor(
    readonly label: string,
    readonly protocol = 'pokecon-json-v1'
  ) {}

  close(): void {
    this.closed.push(true);
    this.readyState = 'closed';
    this.onclose?.call(this.asRtcChannel(), new Event('close'));
  }

  open(): void {
    this.readyState = 'open';
    this.onopen?.call(this.asRtcChannel(), new Event('open'));
  }

  receive(message: ServerMessage | string | ArrayBuffer): void {
    const data = typeof message === 'object' && !(message instanceof ArrayBuffer)
      ? JSON.stringify(message)
      : message;
    this.onmessage?.call(this.asRtcChannel(), new MessageEvent('message', { data }));
  }

  send(data: string): void {
    this.sent.push(data);
  }

  asRtcChannel(): RTCDataChannel {
    return this as unknown as RTCDataChannel;
  }
}

class FakePeer {
  readonly addedIce: RTCIceCandidateInit[] = [];
  readonly addedTransceivers: { kind: string; options?: RTCRtpTransceiverInit }[] = [];
  closed = false;
  connectionState: RTCPeerConnectionState = 'new';
  readonly createdChannels: FakeDataChannel[] = [];
  localDescription: RTCSessionDescription | null = null;
  onconnectionstatechange: RTCPeerConnection['onconnectionstatechange'] = null;
  ondatachannel: RTCPeerConnection['ondatachannel'] = null;
  onicecandidate: RTCPeerConnection['onicecandidate'] = null;
  ontrack: RTCPeerConnection['ontrack'] = null;
  remoteDescription: RTCSessionDescription | null = null;

  constructor(readonly configuration: RTCConfiguration) {}

  addIceCandidate(candidate?: RTCIceCandidateInit | RTCIceCandidate | null): Promise<void> {
    if (candidate !== undefined && candidate !== null) {
      this.addedIce.push(candidate);
    }
    return Promise.resolve();
  }

  addTransceiver(kind: string, options?: RTCRtpTransceiverInit): RTCRtpTransceiver {
    this.addedTransceivers.push({ kind, options });
    return {} as RTCRtpTransceiver;
  }

  close(): void {
    this.closed = true;
    this.connectionState = 'closed';
  }

  createAnswer(): Promise<RTCSessionDescriptionInit> {
    return Promise.resolve({ sdp: 'browser-answer', type: 'answer' });
  }

  createDataChannel(label: string): RTCDataChannel {
    const channel = new FakeDataChannel(label, '');
    this.createdChannels.push(channel);
    return channel.asRtcChannel();
  }

  createOffer(): Promise<RTCSessionDescriptionInit> {
    return Promise.resolve({ sdp: 'browser-offer', type: 'offer' });
  }

  getTransceivers(): RTCRtpTransceiver[] {
    return [];
  }

  setLocalDescription(description?: RTCLocalSessionDescriptionInit): Promise<void> {
    if (description !== undefined) {
      this.localDescription = description as RTCSessionDescription;
    }
    return Promise.resolve();
  }

  setRemoteDescription(description: RTCSessionDescriptionInit): Promise<void> {
    this.remoteDescription = description as RTCSessionDescription;
    return Promise.resolve();
  }

  emitCandidate(candidate: RTCIceCandidateInit): void {
    this.onicecandidate?.call(
      this.asRtcPeer(),
      { candidate: candidate as RTCIceCandidate } as RTCPeerConnectionIceEvent
    );
  }

  emitChannel(channel: FakeDataChannel): void {
    this.ondatachannel?.call(
      this.asRtcPeer(),
      { channel: channel.asRtcChannel() } as RTCDataChannelEvent
    );
  }

  emitVideo(stream: MediaStream): void {
    const track = { kind: 'video' } as MediaStreamTrack;
    this.ontrack?.call(
      this.asRtcPeer(),
      { streams: [stream], track } as unknown as RTCTrackEvent
    );
  }

  asRtcPeer(): RTCPeerConnection {
    return this as unknown as RTCPeerConnection;
  }
}

interface MediaHarness {
  readonly media: MediaTransport;
  readonly peers: FakePeer[];
  readonly realtime: FakeRealtime;
  readonly stream: MediaStream;
  readonly view: () => MediaView;
}

function makeHarness(initialView = realtimeView()): MediaHarness {
  const peers: FakePeer[] = [];
  const realtime = new FakeRealtime(initialView);
  const stream = {} as MediaStream;
  let currentView: MediaView | undefined;
  const dependencies: Partial<MediaDependencies> = {
    createMediaStream: () => stream,
    createPeer: (configuration) => {
      const peer = new FakePeer(configuration);
      peers.push(peer);
      return peer.asRtcPeer();
    },
    now: () => Date.now()
  };
  const media = new MediaTransport(realtime, dependencies);
  media.subscribe((view) => {
    currentView = view;
  });
  return {
    media,
    peers,
    realtime,
    stream,
    view: () => {
      if (currentView === undefined) {
        throw new Error('media view was not published');
      }
      return currentView;
    }
  };
}

async function settle(): Promise<void> {
  await Promise.resolve();
  await Promise.resolve();
  await Promise.resolve();
  await Promise.resolve();
}

function peerAt(peers: readonly FakePeer[], index = 0): FakePeer {
  const peer = peers[index];
  if (peer === undefined) {
    throw new Error(`peer ${String(index)} was not created`);
  }
  return peer;
}

function sentAt(channel: FakeDataChannel, index: number): string {
  const message = channel.sent[index];
  if (message === undefined) {
    throw new Error(`data-channel message ${String(index)} was not sent`);
  }
  return message;
}

function parsedSentAt(channel: FakeDataChannel, index: number): unknown {
  return JSON.parse(sentAt(channel, index)) as unknown;
}

function offer(sdp = 'server-offer'): ServerMessage {
  return { data: { sdp }, type: 'webrtc.offer' };
}

function generation(value = 'generation-1'): ServerMessage {
  return { data: { generation: value }, type: 'input.generation' };
}

function acknowledgement(value = 'generation-1', sequence = '0'): ServerMessage {
  return { data: { generation: value, sequence }, type: 'input.snapshot.applied' };
}

function logMessage(message = 'ready'): ServerMessage {
  return { data: { level: 'info', message, operation: 'append', target: 'log' }, type: 'log' };
}

async function beginNegotiation(harness: MediaHarness): Promise<FakePeer> {
  harness.media.start();
  harness.realtime.emitMessage(offer());
  await settle();
  return peerAt(harness.peers);
}

async function makePrimary(harness: MediaHarness): Promise<{
  readonly control: FakeDataChannel;
  readonly log: FakeDataChannel;
  readonly peer: FakePeer;
}> {
  const peer = await beginNegotiation(harness);
  const control = new FakeDataChannel('pokecon-control');
  const log = new FakeDataChannel('pokecon-log');
  peer.emitVideo(harness.stream);
  peer.emitChannel(control);
  peer.emitChannel(log);
  control.open();
  log.open();
  control.receive(generation());
  control.receive(acknowledgement());
  return { control, log, peer };
}

afterEach(() => {
  vi.useRealTimers();
});

describe('MediaTransport signaling', () => {
  it('queues signaling until settings arrive, uses STUN, flushes ICE, and answers', async () => {
    const initialView = realtimeView({ settings: null, status: 'synchronizing' });
    const harness = makeHarness(initialView);
    harness.media.start();
    harness.realtime.emitMessage({
      data: {
        candidate: 'remote-candidate',
        sdp_mid: 'video',
        sdp_mline_index: 0,
        username_fragment: 'remote-user'
      },
      type: 'webrtc.ice_candidate'
    });
    harness.realtime.emitMessage(offer());

    expect(harness.peers).toHaveLength(0);
    harness.realtime.emitView(realtimeView());
    await settle();

    const peer = peerAt(harness.peers);
    expect(peer.configuration).toEqual({
      iceServers: [{ urls: 'stun:relay.example.test:3478' }]
    });
    expect(peer.remoteDescription).toMatchObject({ sdp: 'server-offer', type: 'offer' });
    expect(peer.addedIce).toEqual([
      {
        candidate: 'remote-candidate',
        sdpMLineIndex: 0,
        sdpMid: 'video',
        usernameFragment: 'remote-user'
      }
    ]);
    expect(harness.realtime.sent).toContainEqual({
      data: { sdp: 'browser-answer' },
      type: 'webrtc.answer'
    });

    peer.emitCandidate({
      candidate: 'local-candidate',
      sdpMLineIndex: 1,
      sdpMid: 'data',
      usernameFragment: 'local-user'
    });
    expect(harness.realtime.sent).toContainEqual({
      data: {
        candidate: 'local-candidate',
        sdp_mid: 'data',
        sdp_mline_index: 1,
        username_fragment: 'local-user'
      },
      type: 'webrtc.ice_candidate'
    });
  });

  it('creates a recvonly manual offer with a bootstrap channel', async () => {
    const harness = makeHarness();
    harness.media.start();
    harness.media.reconnectWebRtc();
    await settle();

    const peer = peerAt(harness.peers);
    expect(peer.addedTransceivers).toEqual([
      { kind: 'video', options: { direction: 'recvonly' } }
    ]);
    expect(peer.createdChannels[0]?.label).toBe('pokecon-bootstrap');
    expect(harness.realtime.sent).toContainEqual({
      data: { sdp: 'browser-offer' },
      type: 'webrtc.offer'
    });
  });
});

describe('MediaTransport routing and failover', () => {
  it('hands the generated route to InputManager before primary promotion', async () => {
    const harness = makeHarness();
    const input = new InputManager(harness.media);
    input.start();
    const peer = await beginNegotiation(harness);
    const control = new FakeDataChannel('pokecon-control');
    const log = new FakeDataChannel('pokecon-log');
    control.readyState = 'open';
    log.readyState = 'open';

    peer.emitVideo(harness.stream);
    peer.emitChannel(control);
    peer.emitChannel(log);
    control.receive(generation());

    expect(parsedSentAt(control, 0)).toMatchObject({
      data: { generation: 'generation-1', sequence: '0' },
      type: 'input.snapshot'
    });
    expect(harness.view().mode).toBe('idle');

    control.receive(acknowledgement());
    expect(harness.view().mode).toBe('webrtc');
    input.setKeyboardKey('KeyA', true);
    expect(parsedSentAt(control, 1)).toEqual({
      data: {
        generation: 'generation-1',
        key: 'KeyA',
        sequence: '1',
        state: 'pressed'
      },
      type: 'keyboard_input'
    });
  });

  it('promotes only after video, exact channels, and the matching snapshot ack', async () => {
    const harness = makeHarness();
    const routes: { message: ServerMessage; route: string }[] = [];
    harness.media.onMessage((route, message) => {
      routes.push({ message, route });
    });
    const peer = await beginNegotiation(harness);
    const control = new FakeDataChannel('pokecon-control');
    const log = new FakeDataChannel('pokecon-log');

    peer.emitVideo(harness.stream);
    peer.emitChannel(control);
    peer.emitChannel(log);
    control.open();
    log.open();
    control.receive(generation());
    control.receive(acknowledgement('other-generation'));
    expect(harness.view()).toMatchObject({ mode: 'idle', negotiating: true, stream: null });

    control.receive(acknowledgement());
    expect(harness.view()).toMatchObject({
      lastError: null,
      mode: 'webrtc',
      negotiating: false,
      stream: harness.stream
    });

    log.receive(logMessage('from rtc'));
    expect(routes).toContainEqual({ message: generation(), route: 'webrtc' });
    expect(routes).toContainEqual({ message: logMessage('from rtc'), route: 'webrtc' });
  });

  it('keeps MJPEG frames and WebSocket input generations on the fallback route', () => {
    const harness = makeHarness();
    const frames: (ArrayBuffer | Blob)[] = [];
    const routes: { message: ServerMessage; route: string }[] = [];
    harness.media.onFallbackFrame((frame) => {
      frames.push(frame);
    });
    harness.media.onMessage((route, message) => {
      routes.push({ message, route });
    });
    harness.media.start();

    const frame = new ArrayBuffer(3);
    harness.realtime.emitFrame(frame);
    harness.realtime.emitMessage(generation('websocket-generation'));

    expect(frames).toEqual([frame]);
    expect(routes).toEqual([
      { message: generation('websocket-generation'), route: 'websocket' }
    ]);
    expect(harness.view().mode).toBe('fallback');
  });

  it('closes the old peer when WebSocket signaling starts a new connection', async () => {
    const harness = makeHarness();
    const { peer } = await makePrimary(harness);

    harness.realtime.emitView(realtimeView({ status: 'connecting' }));

    expect(peer.closed).toBe(true);
    expect(harness.view()).toMatchObject({ mode: 'idle', negotiating: false, stream: null });
  });

  it('rejects invalid channels and channel-specific payloads', async () => {
    const invalidContract = makeHarness();
    const firstPeer = await beginNegotiation(invalidContract);
    const unordered = new FakeDataChannel('pokecon-control');
    unordered.ordered = false;
    firstPeer.emitChannel(unordered);
    expect(firstPeer.closed).toBe(true);
    expect(invalidContract.view()).toMatchObject({
      lastError: 'WebRTC data channel contract is invalid',
      mode: 'fallback'
    });

    const invalidPayload = makeHarness();
    const secondPeer = await beginNegotiation(invalidPayload);
    const log = new FakeDataChannel('pokecon-log');
    secondPeer.emitChannel(log);
    log.open();
    log.receive(generation());
    expect(secondPeer.closed).toBe(true);
    expect(invalidPayload.view().lastError).toContain('unexpected log channel message');
  });

  it('falls back when negotiation exceeds five seconds', async () => {
    vi.useFakeTimers();
    const harness = makeHarness();
    const peer = await beginNegotiation(harness);

    await vi.advanceTimersByTimeAsync(4999);
    expect(peer.closed).toBe(false);
    await vi.advanceTimersByTimeAsync(1);

    expect(peer.closed).toBe(true);
    expect(harness.view()).toMatchObject({
      lastError: 'WebRTC connection timed out',
      mode: 'fallback',
      negotiating: false,
      stream: null
    });
  });

  it('requires activity every three seconds after primary activation', async () => {
    vi.useFakeTimers();
    const harness = makeHarness();
    const { peer } = await makePrimary(harness);
    expect(harness.view().mode).toBe('webrtc');

    await vi.advanceTimersByTimeAsync(2500);
    harness.media.markVideoActivity();
    await vi.advanceTimersByTimeAsync(2999);
    expect(peer.closed).toBe(false);
    await vi.advanceTimersByTimeAsync(1);

    expect(peer.closed).toBe(true);
    expect(harness.view()).toMatchObject({
      lastError: 'WebRTC media became inactive',
      mode: 'fallback',
      stream: null
    });
  });
});
