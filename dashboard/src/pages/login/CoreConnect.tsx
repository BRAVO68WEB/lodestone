import axios from 'axios';
import Button from 'components/Atoms/Button';
import { useContext } from 'react';
import {
  DEFAULT_LOCAL_CORE,
  DISABLE_AUTOFILL,
  errorToString,
  LODESTONE_PORT,
  parseCoreInput,
  coreApiBase,
} from 'utils/util';
import { CoreConnectionInfo, LodestoneContext } from 'data/LodestoneContext';
import InputField from 'components/Atoms/Form/InputField';
import { Form, Formik, FormikHelpers } from 'formik';
import * as yup from 'yup';
import SelectField from 'components/Atoms/Form/SelectField';
import {
  faArrowLeft,
  faArrowRight,
  faDownload,
} from '@fortawesome/free-solid-svg-icons';
import { BrowserLocationContext } from 'data/BrowserLocationContext';
import { CoreInfo, fetchLocalCoreInfo } from 'data/SystemInfo';
import { useDocumentTitle } from 'usehooks-ts';
import WarningAlert from 'components/Atoms/WarningAlert';
import { useEffect, useState } from 'react';
import ConfirmDialog from 'components/Atoms/ConfirmDialog';
import { isIPv6 } from 'is-ip';

const validationSchema = yup.object({
  address: yup.string().required('Required'),
  port: yup.number().required('Required'),
  apiVersion: yup.string().required('Required'),
  protocol: yup.string().required('Required'),
});

const CoreConnect = () => {
  useDocumentTitle('Connect to Core - Lodestone');
  const { navigateBack, setPathname } = useContext(BrowserLocationContext);
  const { setCore, addCore } = useContext(LodestoneContext);
  const [ShowBlurb, setShowBlurb] = useState(false);
  const [showPopup, setShowPopup] = useState(false);
  const [detecting, setDetecting] = useState(false);
  const [detectError, setDetectError] = useState('');
  const [detectedLocal, setDetectedLocal] = useState<CoreInfo | null>(null);

  const initialValues: CoreConnectionInfo = {
    address: '',
    port: LODESTONE_PORT.toString(),
    apiVersion: 'v1',
    protocol: 'http',
  };

  const onSubmit = (
    values: CoreConnectionInfo,
    actions: FormikHelpers<CoreConnectionInfo>
  ) => {
    if (
      isIPv6(values.address) &&
      !values.address.startsWith('[') &&
      !values.address.endsWith(']')
    ) {
      values.address = `[${values.address}]`;
    }
    let core: CoreConnectionInfo;
    try {
      core = parseCoreInput(values.address, values);
    } catch {
      actions.setStatus({ error: 'Enter a host or a full http(s) URL' });
      actions.setSubmitting(false);
      return;
    }
    axios
      .get<CoreInfo>(`/info`, {
        baseURL: coreApiBase(core),
      })
      .then((res) => {
        if (res.status !== 200)
          throw new Error('Invalid response, setup may be invalid');
        setCore(core);
        addCore(core);
        if (res.data.is_setup === false) {
          setPathname('/login/core/first_setup');
        } else {
          setPathname('/login/user/select');
        }
        actions.setSubmitting(false);
        actions.setStatus(null);
      })
      .catch((err) => {
        const errorMessages = errorToString(err);
        const protocolLabel = core.protocol === 'https' ? 'HTTPS' : 'HTTP';
        actions.setStatus({
          error: `${protocolLabel} request failed: ${errorMessages}`,
        });
        actions.setSubmitting(false);
        return;
      });
  };

  useEffect(() => {
    const fetchData = async () => {
      try {
        // http site hosted on kevins arm server
        const response = await fetch('http://132.145.101.179:42069/');
        if (!response.ok) {
          setShowBlurb(true);
        }
      } catch (error) {
        setShowBlurb(true);
      }
    };

    fetchData();
  }, []);

  return (
    <div className="mx-auto flex w-full max-w-[768px] flex-col items-stretch justify-center gap-8 rounded-2xl px-4 py-8 @container sm:gap-12 sm:px-12 sm:py-14">
      {showPopup && (
        <ConfirmDialog
          isOpen={ShowBlurb}
          onClose={() => setShowPopup(false)}
          title="HTTP Error"
          type="danger"
        >
          Missing site permissions. Enable mixed content settings or disable any
          HTTPS browser plugins, then refresh the page.{' '}
          <a
            href="https://github.com/Lodestone-Team/lodestone/wiki/Known-Issues#network-errors"
            target="_blank"
            rel="noreferrer"
            className="text-blue-200 underline hover:text-blue-300"
          >
            Learn more.
          </a>
        </ConfirmDialog>
      )}
      <div className="flex flex-col items-start">
        <img src="/logo.svg" alt="logo" className="h-8" />
        <h1 className="font-title text-h1 font-bold tracking-medium text-gray-300">
          Add a new core
        </h1>
        {ShowBlurb && (
          <WarningAlert>
            <p>
              You may need to adjust your network and browser settings.{' '}
              <a
                href="https://github.com/Lodestone-Team/lodestone/wiki/Known-Issues#network-errors"
                target="_blank"
                rel="noreferrer"
                className="text-blue-200 underline hover:text-blue-300"
              >
                Learn more.
              </a>
            </p>
          </WarningAlert>
        )}
      </div>
      <Formik
        initialValues={initialValues}
        validationSchema={validationSchema}
        onSubmit={onSubmit}
        validateOnBlur={false}
        validateOnChange={false}
      >
        {({ isSubmitting, status, values }) => (
          <Form
            id="addCoreForm"
            className="flex flex-col gap-12"
            autoComplete={DISABLE_AUTOFILL}
          >
            {status && (
              <WarningAlert>
                <p>
                  <b>{status.error}</b>: Please ensure your fields are filled
                  out correctly.
                </p>
              </WarningAlert>
            )}
            <div className="grid grid-cols-1 gap-y-14 gap-x-8 @lg:grid-cols-2">
              <SelectField
                name="apiVersion"
                className="grow"
                options={['v1']}
                label="API Version"
              />
              <SelectField
                name="protocol"
                className="grow"
                options={['http', 'https']}
                label="Protocol"
              />
              <InputField type="number" name="port" label="Port" />
              <InputField
                type="text"
                name="address"
                label="IP Address/Domain"
                placeholder="e.g. 123.123.123.123, myserver.com, or https://core.example.com:16662"
              />
            </div>
            <div className="flex flex-col items-start gap-2">
              <Button
                type="button"
                label={detecting ? 'Looking for a local core' : 'Detect local core'}
                disabled={detecting}
                onClick={() => {
                  setDetectError('');
                  setDetecting(true);
                  fetchLocalCoreInfo()
                    .then((info) => setDetectedLocal(info))
                    .catch((err) => setDetectError(errorToString(err)))
                    .finally(() => setDetecting(false));
                }}
              />
              {detectError && (
                <p className="text-small font-medium text-red-200">
                  No core on localhost:{LODESTONE_PORT}. {detectError}
                </p>
              )}
            </div>
            <ConfirmDialog
              isOpen={detectedLocal !== null}
              title="Use the core on this computer?"
              type="info"
              confirmButtonText="Use local core"
              closeButtonText="Cancel"
              onConfirm={() => {
                const local = { ...DEFAULT_LOCAL_CORE };
                setCore(local);
                addCore(local);
                setDetectedLocal(null);
                if (detectedLocal?.is_setup === false) {
                  setPathname('/login/core/first_setup');
                } else {
                  setPathname('/login/user/select');
                }
              }}
              onClose={() => setDetectedLocal(null)}
            >
              Found {detectedLocal?.core_name ?? 'a core'} at localhost:
              {LODESTONE_PORT}. This replaces the address in the form.
            </ConfirmDialog>
            <div className="flex w-full flex-col gap-3 sm:flex-row sm:justify-between">
              <Button
                type="button"
                icon={faArrowLeft}
                label="Back"
                onClick={navigateBack}
              />
              <div className="flex flex-col gap-3 sm:flex-row">
                <Button
                  type="button"
                  iconRight={faDownload}
                  label="Download Lodestone Core"
                  onClick={() => {
                    window.open(
                      'https://github.com/Lodestone-Team/dashboard/releases/',
                      '_self'
                    );
                  }}
                />
                <Button
                  type="submit"
                  intention="primary"
                  label="Add and Continue"
                  iconRight={faArrowRight}
                  loading={isSubmitting} //TODO: fix button size changing when loading
                  onClick={() => {
                    const address = values.address.trim();
                    const https =
                      values.protocol === 'https' ||
                      /^https:\/\//i.test(address);
                    if (!https) setShowPopup(true);
                  }}
                />
              </div>
            </div>
          </Form>
        )}
      </Formik>
    </div>
  );
};

export default CoreConnect;
