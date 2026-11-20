export type SlotDraft = { id?: string; date: string; start: string; end: string; key: number };
export type Slot = { id: string; date: string; start: string; end: string | null };
export type LocationGeo = {
 latitude: number;
 longitude: number;
 kind: 'address' | 'venue' | 'approximate';
 attribution: string | null;
 place_id?: string;
};
export type EventData = { title: string; description: string | null; location: string; location_geo: LocationGeo | null; organizer_name: string; allow_multiple_choices: boolean; status: 'OPEN'|'CONFIRMED'|'CANCELLED'; confirmed_slot_id: string | null; tiebreak_slot_id: string | null; timezone: string; voting_deadline: string | null; voting_ended: boolean; slots: Slot[]; participants: { name: string; slots: string[] }[] };
export type Question = { title: string; text: string; detail?: string; confirm: string; cancel: string; danger?: boolean };
