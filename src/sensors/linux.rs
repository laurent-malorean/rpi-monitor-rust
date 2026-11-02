//! Capteurs Raspberry Pi / Linux : `/proc`, `/sys`, `vcgencmd`. Compilé sur toutes les
//! plateformes pour que les tests tournent aussi sur Mac.

use super::{FanReading, Sensors};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Puissance estimée (W) : somme tension x courant des rails ayant les deux mesures.
pub fn parse_pmic_power(text: &str) -> Option<f64> {
    let (mut amps, mut volts) = (HashMap::new(), HashMap::new());
    for line in text.lines() {
        let Some((lhs, rhs)) = line.split_once('=') else { continue };
        let mut tok = lhs.split_whitespace();
        let (Some(name), Some(kind)) = (tok.next(), tok.next()) else { continue };
        let Ok(v) = rhs.trim().trim_end_matches(['A', 'V']).parse::<f64>() else { continue };
        if let (Some(rail), true) = (name.strip_suffix("_A"), kind.starts_with("current(")) {
            amps.insert(rail.to_string(), v);
        } else if let (Some(rail), true) = (name.strip_suffix("_V"), kind.starts_with("volt(")) {
            volts.insert(rail.to_string(), v);
        }
    }
    let sum: Vec<f64> = amps.iter().filter_map(|(k, a)| volts.get(k).map(|v| a * v)).collect();
    if sum.is_empty() { None } else { Some(sum.iter().sum()) }
}

fn read_str(path: &Path) -> Option<String> {
    fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}

fn read_int(path: &Path) -> Option<i64> {
    read_str(path)?.parse().ok()
}

fn find_by_file(root: &Path, prefix: &str, file: &str, value: &str) -> Option<PathBuf> {
    let mut dirs: Vec<PathBuf> = fs::read_dir(root).ok()?.flatten().map(|e| e.path()).collect();
    dirs.sort();
    dirs.into_iter().find(|d| {
        d.file_name().is_some_and(|n| n.to_string_lossy().starts_with(prefix))
            && read_str(&d.join(file)).as_deref() == Some(value)
    })
}

/// Chemin hwmon dont le fichier `name` vaut `name` (numérotation non fixe).
pub fn find_hwmon(name: &str, root: &Path) -> Option<PathBuf> {
    find_by_file(root, "hwmon", "name", name)
}

fn find_cooling_device(kind: &str, root: &Path) -> Option<PathBuf> {
    find_by_file(root, "cooling_device", "type", kind)
}

fn find_throttle_file(platform: &Path) -> Option<PathBuf> {
    let level = |dir: &Path| -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> = fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).collect();
        v.sort();
        v
    };
    let first = level(platform);
    first
        .iter()
        .map(|d| d.join("get_throttled"))
        .chain(first.iter().flat_map(|d| level(d)).map(|d| d.join("get_throttled")))
        .find(|f| f.is_file())
}

fn parse_int_auto(raw: &str) -> Option<u32> {
    match raw.strip_prefix("0x") {
        Some(h) => u32::from_str_radix(h, 16).ok(),
        None => raw.parse().ok(),
    }
}

/// Capteurs Pi ; les chemins sont redétectés si une lecture échoue.
pub struct LinuxSensors {
    sys: PathBuf,
    proc: PathBuf,
    real: bool, // vrai `/sys` : seul cas où l'on lance `vcgencmd`
    fan_hwmon: Option<PathBuf>,
    cooling: Option<PathBuf>,
    temp_hwmon: Option<PathBuf>,
    throttle_file: Option<PathBuf>,
    prev_cpu: Option<(u64, u64)>, // (total, idle) ; vide tant qu'aucune mesure n'a servi de départ
    prev_cores: Vec<(u64, u64)>,  // idem pour chaque cœur
}

impl LinuxSensors {
    pub fn new() -> Self {
        Self::with_roots(Path::new("/sys"), Path::new("/proc"))
    }

    /// Racines `/sys` et `/proc` injectables : les tests ne dépendent pas de la machine.
    pub fn with_roots(sys: &Path, proc: &Path) -> Self {
        let mut s = Self {
            sys: sys.to_path_buf(),
            proc: proc.to_path_buf(),
            real: sys == Path::new("/sys"),
            fan_hwmon: None,
            cooling: None,
            temp_hwmon: None,
            throttle_file: None,
            prev_cpu: None,
            prev_cores: vec![],
        };
        s.detect();
        s
    }

    fn detect(&mut self) {
        self.fan_hwmon = find_hwmon("pwmfan", &self.sys.join("class/hwmon"));
        self.temp_hwmon = find_hwmon("cpu_thermal", &self.sys.join("class/hwmon"));
        self.cooling = find_cooling_device("pwm-fan", &self.sys.join("class/thermal"));
        self.throttle_file = find_throttle_file(&self.sys.join("devices/platform"));
    }

    fn read_cpu_times(&self) -> Option<(u64, u64)> {
        let text = fs::read_to_string(self.proc.join("stat")).ok()?;
        let line = text.lines().find(|l| l.starts_with("cpu "))?;
        let v: Vec<u64> = line.split_whitespace().skip(1).take(8).filter_map(|x| x.parse().ok()).collect();
        if v.len() < 5 {
            return None;
        }
        Some((v.iter().sum(), v[3] + v[4])) // idle + iowait
    }

    /// (total, inactif) de chaque ligne `cpuN` de /proc/stat, dans l'ordre.
    fn read_core_times(&self) -> Vec<(u64, u64)> {
        let Ok(text) = fs::read_to_string(self.proc.join("stat")) else { return vec![] };
        text.lines()
            .filter(|l| l.strip_prefix("cpu").is_some_and(|r| r.starts_with(|c: char| c.is_ascii_digit())))
            .filter_map(|l| {
                let v: Vec<u64> = l.split_whitespace().skip(1).take(8).filter_map(|x| x.parse().ok()).collect();
                (v.len() >= 5).then(|| (v.iter().sum(), v[3] + v[4]))
            })
            .collect()
    }

    fn vcgencmd(&self, arg: &str) -> Option<String> {
        if !self.real {
            return None;
        }
        let out = Command::new("vcgencmd").arg(arg).output().ok()?;
        out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
    }
}

impl Default for LinuxSensors {
    fn default() -> Self {
        Self::new()
    }
}

impl Sensors for LinuxSensors {
    fn cpu_percent(&mut self) -> Option<f64> {
        let cur = self.read_cpu_times()?;
        let prev = self.prev_cpu.replace(cur)?; // 1re lecture : point de départ seulement
        let d_total = cur.0.checked_sub(prev.0)?;
        let d_idle = cur.1.saturating_sub(prev.1);
        (d_total > 0).then(|| (d_total - d_idle.min(d_total)) as f64 / d_total as f64 * 100.0)
    }

    fn cpu_cores_percent(&mut self) -> Vec<Option<f64>> {
        let cur = self.read_core_times();
        let prev = std::mem::replace(&mut self.prev_cores, cur.clone());
        cur.iter()
            .enumerate()
            .map(|(i, c)| {
                let p = prev.get(i)?; // 1re lecture : point de départ seulement
                let d_total = c.0.checked_sub(p.0)?;
                let d_idle = c.1.saturating_sub(p.1);
                (d_total > 0).then(|| (d_total - d_idle.min(d_total)) as f64 / d_total as f64 * 100.0)
            })
            .collect()
    }

    fn cpu_freq_mhz(&mut self) -> Option<f64> {
        let dir = self.sys.join("devices/system/cpu/cpu0/cpufreq");
        let khz = read_int(&dir.join("scaling_cur_freq")).or_else(|| read_int(&dir.join("cpuinfo_cur_freq")))?;
        (khz > 0).then(|| khz as f64 / 1000.0)
    }

    fn ram_percent(&mut self) -> Option<f64> {
        let text = fs::read_to_string(self.proc.join("meminfo")).ok()?;
        let field = |name: &str| -> Option<f64> {
            text.lines().find(|l| l.starts_with(name))?.split_whitespace().nth(1)?.parse().ok()
        };
        let (total, avail) = (field("MemTotal:")?, field("MemAvailable:")?);
        (total > 0.0).then(|| (total - avail) / total * 100.0)
    }

    fn temperature_c(&mut self) -> Option<f64> {
        let v = read_int(&self.sys.join("class/thermal/thermal_zone0/temp"))
            .or_else(|| read_int(&self.temp_hwmon.as_ref()?.join("temp1_input")))?;
        Some(v as f64 / 1000.0)
    }

    fn power_w(&mut self) -> Option<f64> {
        parse_pmic_power(&self.vcgencmd("pmic_read_adc")?) // lecture seule, sans sudo
    }

    fn throttled(&mut self) -> Option<u32> {
        let raw = self
            .throttle_file
            .as_deref()
            .and_then(read_str)
            .or_else(|| self.vcgencmd("get_throttled").map(|o| o.rsplit('=').next().unwrap_or("").trim().to_string()))?;
        parse_int_auto(&raw)
    }

    fn fan(&mut self) -> Option<FanReading> {
        let h = self.fan_hwmon.clone();
        let r = FanReading {
            state: self.cooling.as_ref().and_then(|c| read_int(&c.join("cur_state"))),
            pwm: h.as_ref().and_then(|h| read_int(&h.join("pwm1"))),
            rpm: h.as_ref().and_then(|h| read_int(&h.join("fan1_input"))),
        };
        if h.is_none() || (r.pwm.is_none() && r.rpm.is_none()) {
            self.detect(); // périphérique disparu ou renuméroté
        }
        Some(r)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write(p: &Path, content: &str) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, content).unwrap();
    }

    fn tmpdir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("rpimon-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn make_sys(root: &Path, fan_n: u32, pwm: i32, rpm: i32, state: i32) {
        write(&root.join("class/hwmon/hwmon0/name"), "cpu_thermal\n");
        let fan = root.join(format!("class/hwmon/hwmon{fan_n}"));
        write(&fan.join("name"), "pwmfan\n");
        write(&fan.join("pwm1"), &format!("{pwm}\n"));
        write(&fan.join("fan1_input"), &format!("{rpm}\n"));
        let cd = root.join("class/thermal/cooling_device0");
        write(&cd.join("type"), "pwm-fan\n");
        write(&cd.join("cur_state"), &format!("{state}\n"));
        write(&root.join("class/thermal/thermal_zone0/temp"), "48250\n");
    }

    #[test]
    fn hwmon_detected_by_name_not_number() {
        let d = tmpdir("hwmon");
        make_sys(&d, 5, 75, 1700, 1);
        assert_eq!(find_hwmon("pwmfan", &d.join("class/hwmon")).unwrap().file_name().unwrap(), "hwmon5");
    }

    #[test]
    fn linux_sensors_read() {
        let d = tmpdir("read");
        make_sys(&d, 2, 75, 1700, 1);
        let mut s = LinuxSensors::with_roots(&d, &d.join("noproc"));
        assert_eq!(s.temperature_c(), Some(48.25));
        let f = s.fan().unwrap();
        assert_eq!((f.state, f.pwm, f.rpm, f.active()), (Some(1), Some(75), Some(1700), Some(true)));
    }

    #[test]
    fn missing_sensors_do_not_crash() {
        let d = tmpdir("missing");
        let mut s = LinuxSensors::with_roots(&d, &d);
        assert_eq!(s.temperature_c(), None);
        assert_eq!(s.fan().and_then(|f| f.active()), None);
        assert_eq!(s.cpu_percent(), None);
        assert_eq!(s.ram_percent(), None);
        assert_eq!(s.cpu_freq_mhz(), None);
    }

    #[test]
    fn cpu_percent_from_proc_stat_deltas() {
        let d = tmpdir("cpu");
        write(&d.join("stat"), "cpu  100 0 100 800 0 0 0 0 0 0\n");
        let mut s = LinuxSensors::with_roots(&d, &d);
        assert_eq!(s.cpu_percent(), None); // 1re lecture : point de départ seulement
        write(&d.join("stat"), "cpu  200 0 200 900 0 0 0 0 0 0\n");
        // +200 actif, +100 inactif -> 66,7 %
        let v = s.cpu_percent().unwrap();
        assert!((v - 200.0 / 300.0 * 100.0).abs() < 1e-9);
    }

    #[test]
    fn first_cpu_measure_is_never_a_spike_from_startup() {
        // Entre la création et la 1re mesure, quelques jiffies tous « actifs » (démarrage du
        // programme) donneraient 100 % : la 1re lecture ne doit donc rien afficher.
        let d = tmpdir("cpu-spike");
        write(&d.join("stat"), "cpu  100 0 100 800 0 0 0 0 0 0\n");
        let mut s = LinuxSensors::with_roots(&d, &d);
        write(&d.join("stat"), "cpu  102 0 100 800 0 0 0 0 0 0\n");
        assert_eq!(s.cpu_percent(), None);
        write(&d.join("stat"), "cpu  102 0 100 900 0 0 0 0 0 0\n");
        assert_eq!(s.cpu_percent(), Some(0.0));
    }

    #[test]
    fn ram_percent_uses_available_memory() {
        let d = tmpdir("ram");
        write(&d.join("meminfo"), "MemTotal:        1000 kB\nMemFree:          100 kB\nMemAvailable:     250 kB\n");
        let mut s = LinuxSensors::with_roots(&d, &d);
        assert_eq!(s.ram_percent(), Some(75.0));
    }

    #[test]
    fn cpu_freq_from_cpufreq_khz() {
        let d = tmpdir("freq");
        write(&d.join("devices/system/cpu/cpu0/cpufreq/scaling_cur_freq"), "2400000\n");
        let mut s = LinuxSensors::with_roots(&d, &d);
        assert_eq!(s.cpu_freq_mhz(), Some(2400.0));
    }

    const SAMPLE: &str = "\
 VDD_CORE_A current(7)=0.50000000A
 1V8_SYS_A current(2)=1.00000000A
 3V3_NOVOLT_A current(3)=9.00000000A
 VDD_CORE_V volt(15)=2.00000000V
 1V8_SYS_V volt(10)=1.00000000V
 EXT5V_V volt(24)=4.90000000V
";

    #[test]
    fn parse_pmic_power_sums_rails_with_current_and_voltage() {
        // 0.5*2.0 + 1.0*1.0 ; EXT5V (sans courant) et rail sans tension ignorés
        assert_eq!(parse_pmic_power(SAMPLE), Some(2.0));
    }

    #[test]
    fn parse_pmic_power_unusable_output_is_none() {
        assert_eq!(parse_pmic_power(""), None);
        assert_eq!(parse_pmic_power("n'importe quoi"), None);
    }

    #[test]
    fn sensors_power_unavailable_is_none() {
        let d = tmpdir("power");
        assert_eq!(LinuxSensors::with_roots(&d, &d).power_w(), None);
    }

    #[test]
    fn sensors_read_throttled_from_sysfs() {
        let d = tmpdir("thr");
        write(&d.join("devices/platform/soc/soc:firmware/get_throttled"), "0x50005\n");
        assert_eq!(LinuxSensors::with_roots(&d, &d).throttled(), Some(0x50005));
    }

    #[test]
    fn sensors_throttled_unavailable_is_none() {
        let d = tmpdir("thr-none");
        assert_eq!(LinuxSensors::with_roots(&d, &d).throttled(), None);
    }

    #[test]
    fn cpu_cores_percent_per_core_from_proc_stat_deltas() {
        let d = tmpdir("cores");
        let stat = |c: [u64; 4]| {
            // cœur i : actif c[i], inactif 1000 - c[i] sur l'intervalle ; ligne globale « cpu » ignorée
            format!("cpu  1 0 0 1 0 0 0 0 0 0\ncpu0 {} 0 0 {} 0 0 0 0 0 0\ncpu1 {} 0 0 {} 0 0 0 0 0 0\ncpu2 {} 0 0 {} 0 0 0 0 0 0\ncpu3 {} 0 0 {} 0 0 0 0 0 0\nintr 5\n",
                c[0], 1000 - c[0].min(1000), c[1], 1000 - c[1].min(1000), c[2], 1000 - c[2].min(1000), c[3], 1000 - c[3].min(1000))
        };
        write(&d.join("stat"), "cpu  0 0 0 0 0 0 0 0 0 0\ncpu0 0 0 0 0 0 0 0 0 0 0\ncpu1 0 0 0 0 0 0 0 0 0 0\ncpu2 0 0 0 0 0 0 0 0 0 0\ncpu3 0 0 0 0 0 0 0 0 0 0\n");
        let mut s = LinuxSensors::with_roots(&d, &d);
        assert_eq!(s.cpu_cores_percent(), vec![None; 4], "1re lecture : point de départ seulement");
        write(&d.join("stat"), &stat([1000, 500, 0, 250]));
        let v = s.cpu_cores_percent();
        assert_eq!(v.len(), 4);
        let got: Vec<f64> = v.iter().map(|x| x.unwrap()).collect();
        assert_eq!(got, vec![100.0, 50.0, 0.0, 25.0]);
    }

    #[test]
    fn cpu_cores_without_per_core_lines_is_empty() {
        let d = tmpdir("cores-none");
        write(&d.join("stat"), "cpu  100 0 100 800 0 0 0 0 0 0\n");
        assert!(LinuxSensors::with_roots(&d, &d).cpu_cores_percent().is_empty());
        assert!(LinuxSensors::with_roots(&tmpdir("cores-nofile"), &tmpdir("cores-nofile2")).cpu_cores_percent().is_empty());
    }

    #[test]
    fn a_core_with_no_elapsed_time_is_unavailable_not_zero() {
        let d = tmpdir("cores-idle");
        write(&d.join("stat"), "cpu  0 0 0 0 0 0 0 0 0 0\ncpu0 10 0 0 90 0 0 0 0 0 0\n");
        let mut s = LinuxSensors::with_roots(&d, &d);
        s.cpu_cores_percent();
        assert_eq!(s.cpu_cores_percent(), vec![None]);
    }
}
