import java.util.ArrayList;
import java.util.Collections;
import java.util.EnumSet;
import java.util.List;
import java.util.Map;
import java.util.TreeMap;
import org.wpilib.framework.RobotBase;
import org.wpilib.framework.TimedRobot;
import org.wpilib.networktables.MultiSubscriber;
import org.wpilib.networktables.NetworkTableEvent;
import org.wpilib.networktables.NetworkTableInstance;
import org.wpilib.networktables.NetworkTablesJNI;
import org.wpilib.system.RobotController;

/**
 * Reads what HeliOS publishes over NetworkTables and logs it with robot time, one line a second for
 * each topic base (`<prefix>/<pipeline>`) it sees. It drives nothing and never leaves disabled: it
 * has no outputs.
 *
 * <p>A publisher stamps capture_time_us as its estimate of this server's clock, minus latency_ms. So
 * the age of a capture on arrival, less that tick's latency, is the time on the wire plus the error
 * in that estimate: `transit_us`. It can only go below zero through clock error. This WPILib's clocks
 * count nanoseconds (measured on a SystemCore, in step with its uptime); capture_time_us is
 * microseconds of the same clock.
 */
public final class NtCheck extends TimedRobot {
  private static final String PREFIX = System.getProperty("nt.prefix", "/helios/");

  /** What one topic base has said since the last log line. */
  private static final class Base {
    final List<Long> transit = new ArrayList<>();
    long age;
    boolean stamped;
    double latencyMs;
    double fps;
    long tags;
    boolean poseValid;
  }

  private final NetworkTableInstance nt = NetworkTableInstance.getDefault();
  private final MultiSubscriber topics = new MultiSubscriber(nt, new String[] {PREFIX});
  private final Map<String, Base> bases = new TreeMap<>();
  private long nextLogNs;

  public NtCheck() {
    nt.addListener(topics, EnumSet.of(NetworkTableEvent.Kind.VALUE_ALL), this::received);
  }

  private void received(NetworkTableEvent event) {
    long nowUs = NetworkTablesJNI.now() / 1000;
    String name = NetworkTablesJNI.getTopicName(event.valueData.topic);
    int slash = name.lastIndexOf('/');
    String leaf = name.substring(slash + 1);
    var value = event.valueData.value;
    synchronized (bases) {
      Base base = bases.computeIfAbsent(name.substring(0, slash), key -> new Base());
      switch (leaf) {
        case "capture_time_us" -> {
          base.age = nowUs - value.getInteger();
          base.stamped = true;
        }
        // The engine publishes latency_ms right after capture_time_us, from the same tick.
        case "latency_ms" -> {
          base.latencyMs = value.getDouble();
          if (base.stamped) {
            base.transit.add(base.age - (long) (base.latencyMs * 1000));
            base.stamped = false;
          }
        }
        case "fps" -> base.fps = value.getDouble();
        case "tag_count" -> base.tags = value.getInteger();
        case "robot_pose_valid" -> base.poseValid = value.getBoolean();
        default -> { }
      }
    }
  }

  @Override
  public void robotPeriodic() {
    long robotNs = RobotController.getTime();
    if (robotNs < nextLogNs) {
      return;
    }
    nextLogNs = robotNs + 1_000_000_000;
    int clients = nt.getConnections().length;
    synchronized (bases) {
      bases.forEach((name, base) -> {
        List<Long> window = base.transit;
        Collections.sort(window);
        String transit = window.isEmpty()
            ? "none"
            : window.get(0) + "/" + window.get(window.size() / 2) + "/" + window.get(window.size() - 1);
        System.out.printf(
            "nt-check robot_s=%.3f clients=%d base=%s updates=%d transit_us=%s latency_ms=%.2f fps=%.1f tags=%d pose_valid=%b%n",
            robotNs / 1e9, clients, name, window.size(), transit,
            base.latencyMs, base.fps, base.tags, base.poseValid);
        window.clear();
      });
    }
  }

  public static void main(String[] args) {
    RobotBase.startRobot(NtCheck::new);
  }
}
