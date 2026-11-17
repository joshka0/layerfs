//! The kernel mount table as the SDK reads it for the lost-mount check
//! (core/docs/issues/lost-mount/README.md L1): the mount point and device of
//! each row of `/proc/self/mountinfo`.

/// Every row's mount point and device, or `None` when the table cannot be
/// read. Then no Workspace is called lost on the kernel's account: a wrong
/// "lost" would refuse a healthy Workspace (L10).
pub(crate) fn rows() -> Option<Vec<(String, (u32, u32))>> {
    let table = std::fs::read("/proc/self/mountinfo").ok()?;
    Some(table.split(|byte| *byte == b'\n').filter_map(row).collect())
}

fn row(line: &[u8]) -> Option<(String, (u32, u32))> {
    let mut fields = line.split(|byte| *byte == b' ');
    fields.next()?; // mount id
    fields.next()?; // parent id
    let device = std::str::from_utf8(fields.next()?).ok()?;
    let (major, minor) = device.split_once(':')?;
    fields.next()?; // root
    let point = String::from_utf8(unescape(fields.next()?)).ok()?;
    Some((point, (major.parse().ok()?, minor.parse().ok()?)))
}

/// The table writes space, tab, newline and backslash as `\` and three octal digits.
fn unescape(field: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(field.len());
    let mut index = 0;
    while index < field.len() {
        match field.get(index + 1..index + 4) {
            Some(digits)
                if field[index] == b'\\' && digits.iter().all(|d| (b'0'..=b'7').contains(d)) =>
            {
                let value = digits
                    .iter()
                    .fold(0u32, |sum, digit| sum * 8 + u32::from(digit - b'0'));
                out.push(value as u8);
                index += 4;
            }
            _ => {
                out.push(field[index]);
                index += 1;
            }
        }
    }
    out
}
