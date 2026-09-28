CREATE TRIGGER IF NOT EXISTS registrations_require_active_reservation_insert
BEFORE INSERT ON registrations
WHEN NEW.status IN ('registration_pending', 'registered')
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

CREATE TRIGGER IF NOT EXISTS registrations_require_active_reservation_update
BEFORE UPDATE OF status, reservation_id, gate_id, owner_id ON registrations
WHEN NEW.status IN ('registration_pending', 'registered')
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
