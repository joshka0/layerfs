//! Whether another mount namespace's copy of a connection's mount keeps that
//! connection after a plain detach here (lost-mount L9, L11, L13). Pure: it
//! reads mount-table rows given to it and makes no syscall.

/// The propagation fields of one mount-table row: its peer group
/// (`shared:N`) and the peer group it receives from (`master:N`).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Peers {
    pub shared: Option<u64>,
    pub master: Option<u64>,
}

/// The peers among the optional fields of a row (the fields between its
/// mount options and the `-` separator).
pub fn peers<'a>(optional: impl Iterator<Item = &'a [u8]>) -> Peers {
    let number = |field: &[u8], tag: &[u8]| -> Option<u64> {
        std::str::from_utf8(field.strip_prefix(tag)?)
            .ok()?
            .parse()
            .ok()
    };
    let mut found = Peers::default();
    for field in optional {
        if let Some(group) = number(field, b"shared:") {
            found.shared = Some(group);
        } else if let Some(group) = number(field, b"master:") {
            found.master = Some(group);
        }
    }
    found
}

/// The device and peers of one `/proc/<pid>/mountinfo` row; `None` for a row
/// that does not parse.
pub fn row(line: &[u8]) -> Option<((u32, u32), Peers)> {
    let mut fields = line.split(|byte| *byte == b' ');
    fields.next()?; // mount id
    fields.next()?; // parent id
    let device = std::str::from_utf8(fields.next()?).ok()?;
    let (major, minor) = device.split_once(':')?;
    fields.next()?; // root
    fields.next()?; // mount point
    fields.next()?; // mount options
    let mut optional = Vec::new();
    // A row ends its optional fields with `-`; a row without it is cut short.
    loop {
        let field = fields.next()?;
        if field == b"-" {
            break;
        }
        optional.push(field);
    }
    Some((
        (major.parse().ok()?, minor.parse().ok()?),
        peers(optional.into_iter()),
    ))
}

/// True when `line`, a row of another mount namespace, is a copy of the mount
/// with `device` that an unmount of that mount here does not reach. A copy
/// reached by the unmount is a member of the mount's own peer group, or a
/// slave of it. A mount with no peer group of its own reaches no copy.
pub fn pins(own: Peers, device: (u32, u32), line: &[u8]) -> bool {
    let Some((copy_device, copy)) = row(line) else {
        return false;
    };
    if copy_device != device {
        return false;
    }
    match own.shared {
        Some(group) => copy.shared != Some(group) && copy.master != Some(group),
        None => true,
    }
}
