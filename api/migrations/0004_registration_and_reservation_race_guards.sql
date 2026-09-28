DROP TRIGGER IF EXISTS registrations_require_active_reservation_insert;
DROP TRIGGER IF EXISTS registrations_require_active_reservation_update;

CREATE TRIGGER registrations_require_active_reservation_insert
BEFORE INSERT ON registrations
WHEN NEW.status IN ('registration_pending', 'registered', 'failed')
 AND NOT EXISTS (
    SELECT 1 FROM reservations
    WHERE id = NEW.reservation_id
      AND owner_id = NEW.owner_id
      AND gate_id = NEW.gate_id
      AND status = 'active'
 )
BEGIN
    SELECT RAISE(ABORT, 'registration_requires_active_reservation');
END;

CREATE TRIGGER registrations_require_active_reservation_update
BEFORE UPDATE OF status, reservation_id, gate_id, owner_id ON registrations
WHEN NEW.status IN ('registration_pending', 'registered', 'failed')
 AND NOT EXISTS (
    SELECT 1 FROM reservations
    WHERE id = NEW.reservation_id
      AND owner_id = NEW.owner_id
      AND gate_id = NEW.gate_id
      AND status = 'active'
 )
BEGIN
    SELECT RAISE(ABORT, 'registration_requires_active_reservation');
END;

CREATE TRIGGER reservations_require_active_update
BEFORE UPDATE OF starts_at, ends_at ON reservations
WHEN OLD.status <> 'active'
BEGIN
    SELECT RAISE(ABORT, 'reservation_update_requires_active');
END;

CREATE TRIGGER registrations_prevent_duplicate_insert
BEFORE INSERT ON registrations
WHEN NEW.status IN ('registration_pending', 'registered', 'failed')
 AND EXISTS (
    SELECT 1 FROM registrations existing
    WHERE existing.owner_id = NEW.owner_id
      AND existing.credential_id = NEW.credential_id
      AND existing.reservation_id = NEW.reservation_id
      AND existing.gate_id = NEW.gate_id
      AND existing.status IN ('registration_pending', 'registered', 'failed')
 )
BEGIN
    SELECT RAISE(ABORT, 'registration_already_exists');
END;

CREATE TRIGGER registrations_prevent_duplicate_update
BEFORE UPDATE OF status, credential_id, reservation_id, gate_id, owner_id ON registrations
WHEN NEW.status IN ('registration_pending', 'registered', 'failed')
 AND EXISTS (
    SELECT 1 FROM registrations existing
    WHERE existing.id <> NEW.id
      AND existing.owner_id = NEW.owner_id
      AND existing.credential_id = NEW.credential_id
      AND existing.reservation_id = NEW.reservation_id
      AND existing.gate_id = NEW.gate_id
      AND existing.status IN ('registration_pending', 'registered', 'failed')
 )
BEGIN
    SELECT RAISE(ABORT, 'registration_already_exists');
END;
