# Contract Review System - API Documentation

## Overview

The Contract Review System allows users to submit, view, and manage reviews for smart contracts in the Soroban Registry. Reviews include ratings (1.0-5.0), optional text feedback, and helpfulness voting.

## Key Features

- **Rating System**: 1.0 to 5.0 star ratings with one decimal precision
- **Moderation Workflow**: All reviews start as "pending" and require admin approval
- **Helpful Voting**: Users can vote on review helpfulness
- **Duplicate Prevention**: One review per user per contract
- **Verified User Mode**: Optional restriction for verified publishers only
- **Rating Aggregation**: Automatic calculation of average ratings and distribution

## Review Status Flow

```
┌─────────────┐     ┌─────────────┐     ┌─────────────┐
│   PENDING   │────▶│  APPROVED   │     │  REJECTED   │
│  (default)  │     │  (visible)  │     │   (hidden)  │
└─────────────┘     └─────────────┘     └─────────────┘
     │                    ▲
     │                    │
     └────────────────────┘
         (admin action)
```

---

## Endpoints

### 1. Submit a Review

**POST** `/api/contracts/:id/reviews`

Submit a new review for a contract. The review will have `pending` status until approved by an administrator.

#### Authentication
**Required**: Yes (Bearer JWT token)

#### Query Parameters

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `verified_only` | boolean | `false` | If `true`, only users with verified contracts can submit reviews |

#### Request Body

```json
{
  "rating": 4.5,
  "review_text": "Great contract! Very well optimized and easy to integrate.",
  "version": "1.0.0"
}
```

| Field | Type | Required | Constraints | Description |
|-------|------|----------|-------------|-------------|
| `rating` | number | Yes | `1.0 <= rating <= 5.0` | Rating from 1.0 to 5.0 (one decimal allowed) |
| `review_text` | string | No | Max 5000 chars | Optional review text |
| `version` | string | No | Valid semver | Contract version being reviewed |

#### Response

**Status**: `201 Created`

```json
{
  "id": 1,
  "contract_id": "550e8400-e29b-41d4-a716-446655440000",
  "user_id": "550e8400-e29b-41d4-a716-446655440001",
  "version": "1.0.0",
  "rating": 4.5,
  "review_text": "Great contract! Very well optimized and easy to integrate.",
  "helpful_count": 0,
  "is_flagged": false,
  "status": "pending",
  "created_at": "2026-03-28T10:00:00Z",
  "updated_at": null
}
```

#### Error Responses

**400 Bad Request** - Invalid rating or duplicate review
```json
{
  "error": "InvalidRating",
  "message": "Rating must be between 1.0 and 5.0"
}
```

```json
{
  "error": "DuplicateReview",
  "message": "You have already submitted a review for this contract"
}
```

**403 Forbidden** - Verified user required
```json
{
  "error": "VerifiedUserRequired",
  "message": "Only users with verified contracts can submit reviews"
}
```

**404 Not Found** - Contract doesn't exist
```json
{
  "error": "ContractNotFound",
  "message": "Contract with id {id} not found"
}
```

---

### 2. Fetch Reviews

**GET** `/api/contracts/:id/reviews`

Fetch approved reviews for a contract. Only reviews with `approved` status are returned.

#### Authentication
**Required**: No (public endpoint)

#### Query Parameters

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `sort_by` | string | `most_recent` | Sort order: `most_helpful`, `most_recent`, `highest_rated`, `lowest_rated` |
| `limit` | integer | `20` | Maximum reviews to return (max: 100) |
| `offset` | integer | `0` | Offset for pagination |

#### Example Requests

```bash
# Get most recent reviews
GET /api/contracts/{id}/reviews

# Get most helpful reviews
GET /api/contracts/{id}/reviews?sort_by=most_helpful

# Get highest rated reviews with pagination
GET /api/contracts/{id}/reviews?sort_by=highest_rated&limit=10&offset=0
```

#### Response

**Status**: `200 OK`

```json
[
  {
    "id": 1,
    "contract_id": "550e8400-e29b-41d4-a716-446655440000",
    "user_id": "550e8400-e29b-41d4-a716-446655440001",
    "version": "1.0.0",
    "rating": 4.5,
    "review_text": "Great contract!",
    "helpful_count": 12,
    "is_flagged": false,
    "status": "approved",
    "created_at": "2026-03-28T10:00:00Z",
    "updated_at": null
  }
]
```

**Note**: Only `approved` reviews are returned. `pending` and `rejected` reviews are filtered out.

---

### 3. Vote on Review Helpfulness

**POST** `/api/contracts/:id/reviews/:review_id/vote`

Vote on whether a review was helpful. Prevents duplicate votes from the same user.

#### Authentication
**Required**: Yes (Bearer JWT token)

#### Request Body

```json
{
  "helpful": true
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `helpful` | boolean | Yes | `true` = helpful, `false` = unhelpful |

#### Response

**Status**: `200 OK`

```json
{
  "review_id": 1,
  "helpful_count": 5,
  "vote_recorded": true
}
```

| Field | Type | Description |
|-------|------|-------------|
| `review_id` | integer | The review ID |
| `helpful_count` | integer | Updated helpful count |
| `vote_recorded` | boolean | `true` if vote was recorded/changed, `false` if unchanged |

#### Behavior

- **First vote**: Creates vote record, increments `helpful_count` if `helpful=true`
- **Change vote**: Updates vote, adjusts `helpful_count` accordingly
- **Same vote**: No change made, `vote_recorded=false`

#### Error Responses

**404 Not Found** - Review doesn't exist
```json
{
  "error": "ReviewNotFound",
  "message": "Review not found or does not belong to this contract"
}
```

---

### 4. Flag Review for Moderation

**POST** `/api/contracts/:id/reviews/:review_id/flag`

Flag a review for moderator review. Users cannot flag the same review multiple times.

#### Authentication
**Required**: Yes (Bearer JWT token)

#### Request Body

```json
{
  "reason": "Spam or misleading content"
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `reason` | string | Yes | Reason for flagging (max 500 chars) |

#### Response

**Status**: `204 No Content`

#### Error Responses

**400 Bad Request** - Already flagged
```json
{
  "error": "AlreadyFlagged",
  "message": "You have already flagged this review"
}
```

**404 Not Found** - Review doesn't exist
```json
{
  "error": "ReviewNotFound",
  "message": "Review not found or does not belong to this contract"
}
```

---

### 5. Moderate Review (Admin Only)

**POST** `/api/contracts/:id/reviews/:review_id/moderate`

Approve or reject a pending review. Only administrators can access this endpoint.

#### Authentication
**Required**: Yes (Admin JWT token)

#### Request Body

```json
{
  "action": "approve"
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `action` | string | Yes | Action: `"approve"` or `"reject"` |

#### Response

**Status**: `200 OK`

```json
{
  "id": 1,
  "contract_id": "550e8400-e29b-41d4-a716-446655440000",
  "user_id": "550e8400-e29b-41d4-a716-446655440001",
  "version": "1.0.0",
  "rating": 4.5,
  "review_text": "Great contract!",
  "helpful_count": 0,
  "is_flagged": false,
  "status": "approved",
  "created_at": "2026-03-28T10:00:00Z",
  "updated_at": "2026-03-28T11:00:00Z"
}
```

#### Error Responses

**403 Forbidden** - Not an admin
```json
{
  "error": "AdminRequired",
  "message": "Only administrators can moderate reviews"
}
```

**400 Bad Request** - Invalid action
```json
{
  "error": "InvalidAction",
  "message": "Action must be 'approve' or 'reject'"
}
```

---

### 6. Get Rating Statistics

**GET** `/api/contracts/:id/rating-stats`

Get aggregated rating statistics for a contract, including average rating and distribution.

#### Authentication
**Required**: No (public endpoint)

#### Response

**Status**: `200 OK`

```json
{
  "average_rating": 4.3,
  "total_reviews": 42,
  "rating_distribution": {
    "stars_1": 2,
    "stars_2": 3,
    "stars_3": 5,
    "stars_4": 12,
    "stars_5": 20
  }
}
```

| Field | Type | Description |
|-------|------|-------------|
| `average_rating` | number | Average rating (0.0 if no reviews) |
| `total_reviews` | integer | Total approved reviews |
| `rating_distribution` | object | Distribution by star count (null if no reviews) |
| `rating_distribution.stars_1` | integer | Number of 1-star reviews (1.0 <= rating < 2.0) |
| `rating_distribution.stars_2` | integer | Number of 2-star reviews (2.0 <= rating < 3.0) |
| `rating_distribution.stars_3` | integer | Number of 3-star reviews (3.0 <= rating < 4.0) |
| `rating_distribution.stars_4` | integer | Number of 4-star reviews (4.0 <= rating < 5.0) |
| `rating_distribution.stars_5` | integer | Number of 5-star reviews (rating = 5.0) |

---

## Database Schema

### Tables

#### `reviews`

| Column | Type | Description |
|--------|------|-------------|
| `id` | SERIAL | Primary key |
| `contract_id` | UUID | Foreign key to contracts |
| `user_id` | UUID | Reviewer's publisher ID |
| `version` | TEXT | Contract version reviewed |
| `rating` | NUMERIC(2,1) | Rating (1.0-5.0) |
| `review_text` | TEXT | Optional review text |
| `helpful_count` | INT | Net helpful votes |
| `is_flagged` | BOOLEAN | True if flagged for moderation |
| `status` | review_status | `pending`, `approved`, or `rejected` |
| `created_at` | TIMESTAMPTZ | Creation timestamp |
| `updated_at` | TIMESTAMPTZ | Last update timestamp |

#### `review_votes`

| Column | Type | Description |
|--------|------|-------------|
| `id` | SERIAL | Primary key |
| `review_id` | INT | Foreign key to reviews |
| `user_id` | UUID | Voter's publisher ID |
| `vote` | BOOLEAN | `true` = helpful, `false` = unhelpful |
| `created_at` | TIMESTAMPTZ | Vote timestamp |

**Unique Constraint**: `(review_id, user_id)` - One vote per user per review

#### `review_flags`

| Column | Type | Description |
|--------|------|-------------|
| `id` | SERIAL | Primary key |
| `review_id` | INT | Foreign key to reviews |
| `user_id` | UUID | Flagging user's publisher ID |
| `reason` | TEXT | Reason for flagging |
| `resolved` | BOOLEAN | True if moderator has resolved |
| `created_at` | TIMESTAMPTZ | Flag timestamp |

---

## Best Practices

### For Users

1. **Be Constructive**: Provide specific feedback about the contract
2. **One Review Per Contract**: Submit your honest review once
3. **Vote Helpfully**: Mark reviews that provide useful insights
4. **Flag Appropriately**: Only flag reviews that violate guidelines

### For Administrators

1. **Review Promptly**: Approve/reject pending reviews in a timely manner
2. **Be Consistent**: Apply moderation standards uniformly
3. **Check Flags**: Investigate flagged reviews for policy violations
4. **Monitor Patterns**: Watch for spam or abuse patterns

### For Developers

1. **Handle Empty States**: Display appropriate UI when no reviews exist
2. **Show Rating Distribution**: Display the full distribution, not just average
3. **Sort by Default**: Default to "most recent" or "most helpful"
4. **Cache Stats**: Consider caching rating stats for high-traffic contracts

---

## Error Handling

### Common Error Codes

| Code | HTTP Status | Description |
|------|-------------|-------------|
| `InvalidRating` | 400 | Rating outside 1.0-5.0 range |
| `DuplicateReview` | 400 | User already reviewed this contract |
| `AlreadyFlagged` | 400 | User already flagged this review |
| `UserNotFound` | 400 | User doesn't have publisher account |
| `VerifiedUserRequired` | 403 | Verified user mode enabled |
| `AdminRequired` | 403 | Admin access required |
| `ContractNotFound` | 404 | Contract doesn't exist |
| `ReviewNotFound` | 404 | Review doesn't exist |

---

## Testing

Run the review system tests:

```bash
# Start the API server
cargo run --bin api

# Run tests (in another terminal)
cargo test --test review_tests -- --include-ignored
```

Tests cover:
- Rating validation (bounds checking)
- Review submission and retrieval
- Sorting functionality
- Helpful voting
- Review flagging
- Admin moderation
- Rating aggregation
- Duplicate prevention
- Verified user mode
- Edge cases

---

## Security Considerations

1. **Authentication Required**: Most review actions require authenticated users
2. **Rate Limiting**: Review endpoints are subject to rate limiting
3. **Input Validation**: All inputs are validated for type and length
4. **SQL Injection Prevention**: Uses parameterized queries via SQLx
5. **XSS Prevention**: Review text should be sanitized on the frontend

---

## Performance

- **Indexes**: Optimized indexes on `(contract_id, status)` for fast filtering
- **Pagination**: Limit results to prevent large payloads
- **Caching**: Consider caching rating stats for frequently accessed contracts
- **Aggregation**: Rating stats computed dynamically (can be materialized for scale)

---

## Future Enhancements

- [ ] Review editing (for pending reviews)
- [ ] Review deletion (soft delete for admins)
- [ ] Review replies/threaded discussions
- [ ] Image/media attachments in reviews
- [ ] Verified purchase badges
- [ ] Review helpfulness trending
- [ ] Automated spam detection
- [ ] Review notifications for contract owners
