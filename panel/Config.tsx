import { useEffect, useState } from "react";

export default function () {
    const [config, set_config] = useState("");
    const [config_file, set_config_status] = useState<any>({});
    useEffect(() => {
        (async () => {
            const res = await fetch("/api/config");
            set_config(await res.text());
        })();
    }, [])

    useEffect(() => {
        (async () => {
            const res = await fetch("/api/config/status");
            const status = await res.json();
            console.log(status);
            set_config_status(status);
        })();
    }, [])

    return (
        <div style={{ maxHeight: "500px", overflowY: "auto" }}>
            <h1>Time: {new Date(config_file.mtimeMs).toLocaleString()}</h1>
            <pre>{config}</pre>
        </div>
    )
}