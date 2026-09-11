import type {
  InputRoute,
  RoutedMessageSubscriber,
  RoutedMessageTransport
} from './input';
import type { RealtimeClient, RealtimeView } from './realtime';
import {
  parseServerMessage,
  serializeClientMessage,
  type ClientMessage,
  type ServerMessage
} from './wire';

const CONTROL_CHANNEL = 'pokecon-control';
const LOG_CHANNEL = 'pokecon-log';
const CHANNEL_PROTOCOL = 'pokecon-json-v1';
const BOOTSTRAP_CHANNEL = 'pokecon-bootstrap';
const CONNECTION_TIMEOUT_MILLISECONDS = 5000;
const INACTIVITY_TIMEOUT_MILLISECONDS = 3000;
const MAX_BUFFERED_AMOUNT = 1024 * 1024;

export type MediaMode = 'idle' | 'fallback' | 'webrtc';

export interface MediaView {
  readonly lastError: string | null;
  readonly mode: MediaMode;
  readonly negotiating: boolean;
  readonly stream: MediaStream | null;
}

interface WebSocketMediaTransport {
  onFrame(subscriber: (frame: ArrayBuffer | Blob) => void): () => void;
  onMessage(subscriber: (message: ServerMessage) => void): () => void;
  send(message: ClientMessage): boolean;
  subscribe(subscriber: (view: RealtimeView) => void): () => void;
}

export interface MediaDependencies {
  readonly clearTimeout: (timer: ReturnType<typeof setTimeout>) => void;
  readonly createMediaStream: (tracks: MediaStreamTrack[]) => MediaStream;
  readonly createPeer: (configuration: RTCConfiguration) => RTCPeerConnection;
  readonly now: () => number;
  readonly setTimeout: (
    callback: () => void,
    delayMilliseconds: number
  ) => ReturnType<typeof setTimeout>;
}

type MediaSubscriber = (view: MediaView) => void;
type FallbackFrameSubscriber = (frame: ArrayBuffer | Blob) => void;

const defaultDependencies: MediaDependencies = {
  clearTimeout: (timer) => {
    clearTimeout(timer);
  },
  createMediaStream: (tracks) => new MediaStream(tracks),
  createPeer: (configuration) => new RTCPeerConnection(configuration),
  now: () => performance.now(),
  setTimeout: (callback, delayMilliseconds) => setTimeout(callback, delayMilliseconds)
};

function safeError(error: unknown): string {
  const message = error instanceof Error ? error.message : 'WebRTC operation failed';
  return message.slice(0, 512);
}

function codecRank(codec: { readonly mimeType: string }): number {
  switch (codec.mimeType.toLowerCase()) {
    case 'video/h264':
      return 0;
    case 'video/vp8':
      return 1;
    case 'video/vp9':
      return 2;
    default:
      return 3;
  }
}

function preferVideoCodecs(peer: RTCPeerConnection): void {
  if (
    typeof RTCRtpReceiver === 'undefined' ||
    typeof RTCRtpReceiver.getCapabilities !== 'function'
  ) {
    return;
  }
  const capabilities = RTCRtpReceiver.getCapabilities('video');
  if (capabilities === null) {
    return;
  }
  const codecs = [...capabilities.codecs].sort((left, right) => codecRank(left) - codecRank(right));
  for (const transceiver of peer.getTransceivers()) {
    if (transceiver.receiver.track.kind !== 'video') {
      continue;
    }
    try {
      transceiver.setCodecPreferences(codecs);
    } catch {
      // Older supported engines may expose the method before implementing it.
    }
  }
}

function validDataChannel(channel: RTCDataChannel): boolean {
  return (
    channel.protocol === CHANNEL_PROTOCOL &&
    channel.ordered &&
    channel.maxPacketLifeTime === null &&
    channel.maxRetransmits === null
  );
}

export class MediaTransport implements RoutedMessageTransport {
  private bootstrapChannel: RTCDataChannel | undefined;
  private connectionTimer: ReturnType<typeof setTimeout> | undefined;
  private controlChannel: RTCDataChannel | undefined;
  private controlReady = false;
  private readonly dependencies: MediaDependencies;
  private readonly fallbackSubscribers = new Set<FallbackFrameSubscriber>();
  private inactivityTimer: ReturnType<typeof setTimeout> | undefined;
  private lastActivity = 0;
  private logChannel: RTCDataChannel | undefined;
  private logReady = false;
  private readonly mediaSubscribers = new Set<MediaSubscriber>();
  private readonly messageSubscribers = new Set<RoutedMessageSubscriber>();
  private peer: RTCPeerConnection | undefined;
  private peerToken = 0;
  private pendingIce: RTCIceCandidateInit[] = [];
  private pendingOffer: string | undefined;
  private pendingStream: MediaStream | null = null;
  private realtimeView: RealtimeView | undefined;
  private rtcGeneration: string | null = null;
  private rtcInputReady = false;
  private unsubscribers: (() => void)[] = [];
  private videoReady = false;
  private view: MediaView = {
    lastError: null,
    mode: 'idle',
    negotiating: false,
    stream: null
  };

  constructor(
    private readonly realtime: WebSocketMediaTransport,
    dependencies: Partial<MediaDependencies> = {}
  ) {
    this.dependencies = { ...defaultDependencies, ...dependencies };
  }

  static fromRealtimeClient(
    realtime: RealtimeClient,
    dependencies: Partial<MediaDependencies> = {}
  ): MediaTransport {
    return new MediaTransport(realtime, dependencies);
  }

  subscribe(subscriber: MediaSubscriber): () => void {
    this.mediaSubscribers.add(subscriber);
    subscriber(this.view);
    return () => {
      this.mediaSubscribers.delete(subscriber);
    };
  }

  onFallbackFrame(subscriber: FallbackFrameSubscriber): () => void {
    this.fallbackSubscribers.add(subscriber);
    return () => {
      this.fallbackSubscribers.delete(subscriber);
    };
  }

  onMessage(subscriber: RoutedMessageSubscriber): () => void {
    this.messageSubscribers.add(subscriber);
    return () => {
      this.messageSubscribers.delete(subscriber);
    };
  }

  start(): void {
    if (this.unsubscribers.length > 0) {
      return;
    }
    this.unsubscribers = [
      this.realtime.onMessage((message) => {
        this.handleWebSocketMessage(message);
      }),
      this.realtime.onFrame((frame) => {
        this.activateFallback(null);
        for (const subscriber of this.fallbackSubscribers) {
          subscriber(frame);
        }
      }),
      this.realtime.subscribe((view) => {
        this.handleRealtimeView(view);
      })
    ];
  }

  stop(): void {
    for (const unsubscribe of this.unsubscribers) {
      unsubscribe();
    }
    this.unsubscribers = [];
    this.pendingOffer = undefined;
    this.pendingIce = [];
    this.closePeer();
    this.updateView({ lastError: null, mode: 'idle', negotiating: false, stream: null });
  }

  send(route: InputRoute, message: ClientMessage): boolean {
    if (route === 'websocket') {
      return this.realtime.send(message);
    }
    const channel = this.controlChannel;
    const payload = serializeClientMessage(message);
    const payloadBytes = new TextEncoder().encode(payload).byteLength;
    if (
      channel?.readyState !== 'open' ||
      channel.bufferedAmount + payloadBytes > MAX_BUFFERED_AMOUNT
    ) {
      this.failPeer('WebRTC control channel is unavailable', this.peerToken);
      return false;
    }
    try {
      channel.send(payload);
      return true;
    } catch (error: unknown) {
      this.failPeer(safeError(error), this.peerToken);
      return false;
    }
  }

  markVideoActivity(): void {
    if (this.view.mode !== 'webrtc') {
      return;
    }
    this.lastActivity = this.dependencies.now();
    this.scheduleInactivityTimeout();
  }

  reconnectWebRtc(): void {
    if (this.realtimeView?.status !== 'connected') {
      this.updateView({ lastError: 'WebSocket signaling is not connected' });
      return;
    }
    if (this.view.mode === 'webrtc' || this.view.negotiating) {
      return;
    }
    void this.createManualOffer();
  }

  private handleRealtimeView(view: RealtimeView): void {
    this.realtimeView = view;
    if (
      view.status === 'idle' ||
      view.status === 'connecting' ||
      view.status === 'waiting' ||
      view.status === 'exhausted'
    ) {
      this.pendingOffer = undefined;
      this.pendingIce = [];
      this.closePeer();
      this.updateView({ mode: 'idle', negotiating: false, stream: null });
      return;
    }
    if (view.settings !== null && this.pendingOffer !== undefined) {
      const offer = this.pendingOffer;
      this.pendingOffer = undefined;
      void this.acceptOffer(offer);
    }
  }

  private handleWebSocketMessage(message: ServerMessage): void {
    switch (message.type) {
      case 'webrtc.offer':
        if (this.realtimeView?.settings === null || this.realtimeView?.settings === undefined) {
          this.pendingOffer = message.data.sdp;
        } else {
          void this.acceptOffer(message.data.sdp);
        }
        return;
      case 'webrtc.answer':
        void this.acceptAnswer(message.data.sdp);
        return;
      case 'webrtc.ice_candidate':
        void this.acceptIceCandidate({
          candidate: message.data.candidate,
          sdpMLineIndex: message.data.sdp_mline_index,
          sdpMid: message.data.sdp_mid,
          usernameFragment: message.data.username_fragment
        });
        return;
      case 'input.generation':
        if (this.peer !== undefined) {
          this.closePeer();
        }
        this.activateFallback(null);
        break;
      default:
        break;
    }
    this.publishMessage('websocket', message);
  }

  private async acceptOffer(sdp: string): Promise<void> {
    const { peer, token } = this.beginPeer();
    try {
      await peer.setRemoteDescription({ sdp, type: 'offer' });
      preferVideoCodecs(peer);
      await this.flushIce(peer, token);
      const answer = await peer.createAnswer();
      await peer.setLocalDescription(answer);
      if (!this.isCurrentPeer(peer, token)) {
        return;
      }
      const localSdp = peer.localDescription?.sdp;
      if (localSdp === undefined || !this.realtime.send({ data: { sdp: localSdp }, type: 'webrtc.answer' })) {
        throw new Error('WebRTC answer could not be sent');
      }
    } catch (error: unknown) {
      this.failPeer(safeError(error), token);
    }
  }

  private async createManualOffer(): Promise<void> {
    this.pendingIce = [];
    const { peer, token } = this.beginPeer();
    try {
      peer.addTransceiver('video', { direction: 'recvonly' });
      this.bootstrapChannel = peer.createDataChannel(BOOTSTRAP_CHANNEL, { ordered: true });
      preferVideoCodecs(peer);
      const offer = await peer.createOffer();
      await peer.setLocalDescription(offer);
      if (!this.isCurrentPeer(peer, token)) {
        return;
      }
      const localSdp = peer.localDescription?.sdp;
      if (localSdp === undefined || !this.realtime.send({ data: { sdp: localSdp }, type: 'webrtc.offer' })) {
        throw new Error('WebRTC offer could not be sent');
      }
    } catch (error: unknown) {
      this.failPeer(safeError(error), token);
    }
  }

  private async acceptAnswer(sdp: string): Promise<void> {
    const peer = this.peer;
    const token = this.peerToken;
    if (peer === undefined) {
      return;
    }
    try {
      await peer.setRemoteDescription({ sdp, type: 'answer' });
      await this.flushIce(peer, token);
    } catch (error: unknown) {
      this.failPeer(safeError(error), token);
    }
  }

  private async acceptIceCandidate(candidate: RTCIceCandidateInit): Promise<void> {
    const peer = this.peer;
    const token = this.peerToken;
    if (peer?.remoteDescription == null) {
      this.pendingIce.push(candidate);
      return;
    }
    try {
      await peer.addIceCandidate(candidate);
    } catch (error: unknown) {
      this.failPeer(safeError(error), token);
    }
  }

  private async flushIce(peer: RTCPeerConnection, token: number): Promise<void> {
    const candidates = this.pendingIce;
    this.pendingIce = [];
    for (const candidate of candidates) {
      if (this.peer !== peer || this.peerToken !== token) {
        return;
      }
      await peer.addIceCandidate(candidate);
    }
  }

  private beginPeer(): { peer: RTCPeerConnection; token: number } {
    const previousMode = this.view.mode;
    this.closePeer();
    const token = this.peerToken + 1;
    this.peerToken = token;
    const stunServer = this.realtimeView?.settings?.values.stun_server ?? '';
    const peer = this.dependencies.createPeer({
      iceServers: stunServer === '' ? [] : [{ urls: stunServer }]
    });
    this.peer = peer;
    this.installPeerHandlers(peer, token);
    this.updateView({
      lastError: null,
      mode: previousMode === 'idle' ? 'idle' : 'fallback',
      negotiating: true,
      stream: null
    });
    this.connectionTimer = this.dependencies.setTimeout(() => {
      this.connectionTimer = undefined;
      if (!this.primaryReady()) {
        this.failPeer('WebRTC connection timed out', token);
      }
    }, CONNECTION_TIMEOUT_MILLISECONDS);
    return { peer, token };
  }

  private installPeerHandlers(peer: RTCPeerConnection, token: number): void {
    peer.onicecandidate = (event) => {
      if (this.peer !== peer || event.candidate === null) {
        return;
      }
      const candidate = event.candidate;
      if (
        !this.realtime.send({
          data: {
            candidate: candidate.candidate,
            sdp_mid: candidate.sdpMid ?? null,
            sdp_mline_index: candidate.sdpMLineIndex ?? null,
            username_fragment: candidate.usernameFragment ?? null
          },
          type: 'webrtc.ice_candidate'
        })
      ) {
        this.failPeer('WebRTC ICE candidate could not be sent', token);
      }
    };
    peer.onconnectionstatechange = () => {
      if (
        this.peer === peer &&
        (peer.connectionState === 'failed' ||
          peer.connectionState === 'disconnected' ||
          peer.connectionState === 'closed')
      ) {
        this.failPeer(`WebRTC connection ${peer.connectionState}`, token);
      }
    };
    peer.ondatachannel = (event) => {
      this.installDataChannel(event.channel, peer, token);
    };
    peer.ontrack = (event) => {
      if (this.peer !== peer || event.track.kind !== 'video') {
        return;
      }
      this.pendingStream = event.streams[0] ?? this.dependencies.createMediaStream([event.track]);
      this.videoReady = true;
      this.recordActivity();
      this.activatePrimaryIfReady();
    };
  }

  private installDataChannel(
    channel: RTCDataChannel,
    peer: RTCPeerConnection,
    token: number
  ): void {
    if (!validDataChannel(channel)) {
      channel.close();
      this.failPeer('WebRTC data channel contract is invalid', token);
      return;
    }
    if (channel.label === CONTROL_CHANNEL && this.controlChannel === undefined) {
      this.controlChannel = channel;
      this.installChannelLifecycle(channel, peer, token, 'control');
      return;
    }
    if (channel.label === LOG_CHANNEL && this.logChannel === undefined) {
      this.logChannel = channel;
      this.installChannelLifecycle(channel, peer, token, 'log');
      return;
    }
    channel.close();
    this.failPeer('WebRTC data channel contract is invalid', token);
  }

  private installChannelLifecycle(
    channel: RTCDataChannel,
    peer: RTCPeerConnection,
    token: number,
    kind: 'control' | 'log'
  ): void {
    channel.binaryType = 'arraybuffer';
    channel.onopen = () => {
      if (this.peer !== peer) {
        return;
      }
      if (kind === 'control') {
        this.controlReady = true;
      } else {
        this.logReady = true;
      }
      this.activatePrimaryIfReady();
    };
    channel.onclose = () => {
      if (this.peer === peer) {
        this.failPeer(`WebRTC ${kind} channel closed`, token);
      }
    };
    channel.onerror = () => {
      if (this.peer === peer) {
        this.failPeer(`WebRTC ${kind} channel failed`, token);
      }
    };
    channel.onmessage = (event) => {
      if (this.peer !== peer || typeof event.data !== 'string') {
        this.failPeer(`WebRTC ${kind} channel sent an invalid payload`, token);
        return;
      }
      try {
        const message = parseServerMessage(event.data);
        if (
          (kind === 'log' && message.type !== 'log') ||
          (kind === 'control' &&
            message.type !== 'input.generation' &&
            message.type !== 'input.snapshot.applied')
        ) {
          throw new Error(`unexpected ${kind} channel message`);
        }
        this.recordActivity();
        if (message.type === 'input.generation') {
          this.rtcGeneration = message.data.generation;
          this.rtcInputReady = false;
        } else if (
          message.type === 'input.snapshot.applied' &&
          message.data.generation === this.rtcGeneration &&
          message.data.sequence === '0'
        ) {
          this.rtcInputReady = true;
        }
        this.publishMessage('webrtc', message);
        this.activatePrimaryIfReady();
      } catch (error: unknown) {
        this.failPeer(safeError(error), token);
      }
    };
    if (channel.readyState === 'open') {
      channel.onopen(new Event('open'));
    }
  }

  private isCurrentPeer(peer: RTCPeerConnection, token: number): boolean {
    return this.peer === peer && this.peerToken === token;
  }

  private primaryReady(): boolean {
    return this.videoReady && this.controlReady && this.logReady && this.rtcInputReady;
  }

  private activatePrimaryIfReady(): void {
    if (!this.primaryReady() || this.pendingStream === null) {
      return;
    }
    this.clearConnectionTimer();
    this.bootstrapChannel?.close();
    this.bootstrapChannel = undefined;
    this.lastActivity = this.dependencies.now();
    this.updateView({
      lastError: null,
      mode: 'webrtc',
      negotiating: false,
      stream: this.pendingStream
    });
    this.scheduleInactivityTimeout();
  }

  private recordActivity(): void {
    this.lastActivity = this.dependencies.now();
    if (this.view.mode === 'webrtc') {
      this.scheduleInactivityTimeout();
    }
  }

  private scheduleInactivityTimeout(): void {
    this.clearInactivityTimer();
    const deadline = this.lastActivity + INACTIVITY_TIMEOUT_MILLISECONDS;
    const delay = Math.max(0, deadline - this.dependencies.now());
    const token = this.peerToken;
    this.inactivityTimer = this.dependencies.setTimeout(() => {
      this.inactivityTimer = undefined;
      if (this.dependencies.now() >= deadline) {
        this.failPeer('WebRTC media became inactive', token);
      } else {
        this.scheduleInactivityTimeout();
      }
    }, delay);
  }

  private activateFallback(error: string | null): void {
    this.updateView({
      lastError: error ?? this.view.lastError,
      mode: 'fallback',
      negotiating: this.peer !== undefined,
      stream: null
    });
  }

  private failPeer(message: string, token: number): void {
    if (token !== this.peerToken) {
      return;
    }
    this.closePeer();
    this.activateFallback(message);
  }

  private closePeer(): void {
    this.clearConnectionTimer();
    this.clearInactivityTimer();
    const control = this.controlChannel;
    const log = this.logChannel;
    const bootstrap = this.bootstrapChannel;
    this.controlChannel = undefined;
    this.logChannel = undefined;
    this.bootstrapChannel = undefined;
    for (const channel of [control, log, bootstrap]) {
      if (channel !== undefined) {
        channel.onclose = null;
        channel.onerror = null;
        channel.onmessage = null;
        channel.onopen = null;
        channel.close();
      }
    }
    const peer = this.peer;
    this.peer = undefined;
    if (peer !== undefined) {
      peer.onconnectionstatechange = null;
      peer.ondatachannel = null;
      peer.onicecandidate = null;
      peer.ontrack = null;
      peer.close();
    }
    this.controlReady = false;
    this.logReady = false;
    this.videoReady = false;
    this.rtcGeneration = null;
    this.rtcInputReady = false;
    this.pendingStream = null;
  }

  private publishMessage(route: InputRoute, message: ServerMessage): void {
    for (const subscriber of this.messageSubscribers) {
      subscriber(route, message);
    }
  }

  private updateView(update: Partial<MediaView>): void {
    this.view = { ...this.view, ...update };
    for (const subscriber of this.mediaSubscribers) {
      subscriber(this.view);
    }
  }

  private clearConnectionTimer(): void {
    if (this.connectionTimer !== undefined) {
      this.dependencies.clearTimeout(this.connectionTimer);
      this.connectionTimer = undefined;
    }
  }

  private clearInactivityTimer(): void {
    if (this.inactivityTimer !== undefined) {
      this.dependencies.clearTimeout(this.inactivityTimer);
      this.inactivityTimer = undefined;
    }
  }
}
