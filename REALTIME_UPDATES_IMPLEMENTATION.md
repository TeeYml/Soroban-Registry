# Real-time Contract Updates via WebSocket - Implementation Guide

## Overview
Implemented real-time contract deployment and update notifications via WebSocket, with desktop notifications, automatic reconnection, and user preferences.

---

## Frontend Implementation

### 1. **WebSocket Service** (`services/websocket.service.ts`)
- Singleton WebSocket client with automatic reconnection logic
- Exponential backoff retry strategy (max 10 attempts)
- Ping/pong keep-alive mechanism (30-second intervals)
- Event subscription system with unsubscribe support
- Features:
  - `connect()`: Establish WebSocket connection
  - `on(type, handler)`: Subscribe to specific event types
  - `send(message)`: Send messages to server
  - `disconnect()`: Clean shutdown

### 2. **Realtime Provider** (`providers/RealtimeProvider.tsx`)
- React Context wrapper for WebSocket connection
- Manages notification state (unread count, notifications list)
- Automatically emits desktop notifications based on user preferences
- Interfaces with localStorage for notification settings
- Features:
  - Tracks connection status
  - Stores last 50 notifications
  - Automatic desktop notification delivery
  - Integrates with `useNotificationPreferences` hook

### 3. **Realtime Hook** (`hooks/useRealtime.ts`)
- Simple context consumer hook for accessing realtime features
- Provides access to:
  - `isConnected`: WebSocket connection status
  - `unreadCount`: Number of unread notifications
  - `notifications`: List of recent notifications
  - `subscribe()`: Manual event subscription
  - `clearNotifications()`: Clear notification list
  - `markAsRead()`: Mark notifications as read

### 4. **Notification Preferences** (`hooks/useNotificationPreferences.ts`)
- Manages user notification settings in localStorage
- Persists preferences automatically
- Features:
  - `enableDeploymentNotifications`: Toggle new contract notifications
  - `enableUpdateNotifications`: Toggle contract update notifications
  - `enableDesktopNotifications`: Toggle all desktop notifications
  - `enableSoundNotification`: Toggle sound alerts
  - `updateTypes`: Filter which update types to notify about

### 5. **Desktop Notifications** (`utils/notifications.ts`)
- Wrapper around Notifications API
- Service Worker integration for background notifications
- Features:
  - `requestDesktopNotification()`: Request user permission
  - `sendDesktopNotification()`: Send browser notification
  - `getWebsiteNotificationPermission()`: Check current permission status

### 6. **Notification UI Components**

#### NotificationBell Component
- Navigation bar integration
- Badge showing unread count
- Dropdown list of recent notifications
- Clear all functionality
- Subscribe to Realtime context

#### NotificationPreferencesPanel Component
- Settings UI for notification preferences
- Conditional toggles based on permissions
- Update type filtering
- Sound notification option

### 7. **Auto-Refresh Hook** (`hooks/useContractAutoRefresh.ts`)
- Subscribes to contract updates via WebSocket
- Automatically invalidates React Query caches
- Triggers refetches on events:
  - `contract_deployed`: New contract version
  - `contract_updated`: Security audit, verification, deprecation, breaking changes
- Integrated into contract detail pages

### 8. **Integration**
- Added `RealtimeProvider` to `Providers.tsx` (wraps Providers)
- Added `NotificationBell` to navigation
- Added `useContractAutoRefresh(id)` to contract detail pages
- All components properly wrapped in Context

---

## Backend Implementation

### 1. **WebSocket Module** (`src/websocket.rs`)
- Axum WebSocket handler
- Bidirectional message support
- Broadcast event forwarding
- Connection lifecycle management:
  - Opens: Subscribe to event broadcaster
  - Messages: Handle ping/pong keep-alives
  - Closes: Clean disconnect

### 2. **Realtime Events** (`src/events.rs`)
- Event emission helpers
- Two event types:
  1. **ContractDeployed**: Emitted when new contract published
     - contract_id, contract_name, publisher, version, timestamp
  2. **ContractUpdated**: Emitted on contract changes
     - contract_id, update_type, details, timestamp
- Functions:
  - `emit_contract_deployment()`: Send deployment event
  - `emit_contract_update()`: Send update event

### 3. **Application State** (`src/state.rs`)
- Extended `AppState` with `broadcast::Sender<RealtimeEvent>`
- Created 100-capacity broadcast channel
- Serializable `RealtimeEvent` enum with JSON support

### 4. **WebSocket Routes** (`src/routes.rs`)
- Route: `GET /ws/contracts`
- Handlers websocket upgrade requests
- Integrates into main router

### 5. **Event Integration** (`src/handlers.rs`)
- `publish_contract()` handler now emits `ContractDeployed` event
- Includes contract metadata (name, publisher, version)
- Non-blocking event emission (uses `let _`)

### 6. **Dependencies**
- `tokio-tungstenite` v0.24: WebSocket support

---

## Data Flow

### Deployment Workflow
```
Client: POST /api/contracts (publish)
        ↓
Server: Create contract in DB
        ↓
Server: emit_contract_deployment()
        ↓
Broadcast: Event in channel
        ↓
All WebSocket Clients: Receive event
        ↓
Client: Check notification preferences (localStorage)
        ↓
Client: sendDesktopNotification()
        ↓
Client: Append to notifications list (UI updates)
```

### Real-time Updates
```
Browsers: Connected to /ws/contracts
        ↓
Broadcast: Events flow to all connected clients
        ↓
React Query: Queries invalidated
        ↓
Contracts: Auto-refreshed from API
        ↓
UI: Updated with latest data
```

---

## User Experience

### Connection Lifecycle
1. **Page Load**: Realtime Provider requests notification permission
2. **Automatic Connection**: WebSocket connects to `/ws/contracts`
3. **Connection Lost**: Automatic reconnection with exponential backoff
4. **Events**: Desktop and in-app notifications shown
5. **Cleanup**: On page unload, WebSocket closes gracefully

### Notification Behavior
- Desktop notifications appear even when browser is backgrounded
- Badge appears in navbar showing unread count
- Dropdown shows recent notifications (last 50)
- Preferences respected (deployments, updates, update types)
- Notifications timed out automatically (5 seconds default)

### Preferences Storage
- Stored in browser localStorage
- Key: `notification-preferences`
- Format: JSON object persisting user choices
- Survives page reloads

---

## Configuration

### Frontend Environment
- `NEXT_PUBLIC_API_URL`: Base API URL (defaults to `http://localhost:3001`)
- WebSocket URL derived from API URL (http → ws, https → wss)

### Backend Configuration
- PORT: Server port (default 3001)
- Broadcast channel capacity: 100 events

---

## Testing Checklist

- [ ] WebSocket connects on page load
- [ ] Desktop notifications request permission
- [ ] Publish contract triggers deployment notification
- [ ] Notification appears in dropdown
- [ ] Badge shows correct unread count
- [ ] Clear all button clears notifications
- [ ] Preference toggles persist after reload
- [ ] Contract details auto-refresh on update event
- [ ] Connection reconnects after network loss
- [ ] Notifications respect permission settings

---

## Future Enhancements

1. **Persistence**: Store notifications in backend database
2. **Filtering**: Server-side filtering by user subscriptions
3. **Sound Alerts**: Implement audio notification playback
4. **Analytics**: Track notification engagement
5. **Channels**: Subscribe to specific contract updates
6. **Presence**: Show active users viewing contracts
7. **Typing Indicators**: Show who's modifying contracts
8. **Message History**: Retrieve missed notifications on reconnect
